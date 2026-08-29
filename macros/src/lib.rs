//! Proc-macro crate for type-safe MCP tool registration.
//!
//! Provides the `#[mcp_tool]` attribute macro that generates compile-time
//! tool registration from function signatures.

use proc_macro::TokenStream;
use quote::{quote, format_ident};
use syn::{parse_macro_input, ItemFn, FnArg, Pat, Type, LitStr};

/// Attribute macro for type-safe MCP tool registration.
///
/// Generates a `Tool` struct and registration code from a function signature.
/// The function parameters are converted to a JSON schema automatically.
///
/// # Example
///
/// ```ignore
/// #[mcp_tool(name = "read_file", description = "Read a file from disk")]
/// fn read_file(path: String, offset: Option<i64>) -> Result<String, String> {
///     // implementation
/// }
/// ```
///
/// This generates:
/// - A `Tool` struct with the correct JSON schema
/// - Registration code that adds the tool to the registry
/// - Dispatch code that parses arguments and calls the function
#[proc_macro_attribute]
pub fn mcp_tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attrs = parse_macro_input!(attr as McpToolAttrs);
    let func = parse_macro_input!(item as ItemFn);
    
    let func_name = &func.sig.ident;
    let tool_name = attrs.name.unwrap_or_else(|| func_name.to_string());
    let description = attrs.description.unwrap_or_else(|| format!("Tool: {}", tool_name));
    
    // Extract function components for reconstruction
    let func_params: Vec<_> = func.sig.inputs.iter().collect();
    let return_type = match &func.sig.output {
        syn::ReturnType::Default => quote! { -> () },
        syn::ReturnType::Type(_, ty) => quote! { -> #ty },
    };
    let func_body = &func.block;
    
    // Extract parameters and generate JSON schema
    let mut schema_props = Vec::new();
    let mut required_params = Vec::new();
    let mut param_extractions = Vec::new();
    let mut param_names = Vec::new();
    
    for arg in &func.sig.inputs {
        if let FnArg::Typed(pat_type) = arg {
            if let Pat::Ident(pat_ident) = &*pat_type.pat {
                let param_name = pat_ident.ident.to_string();
                let param_type = &*pat_type.ty;
                let param_ident = &pat_ident.ident;
                
                param_names.push(param_ident.clone());
                
                // Check if it's Option<T>
                let is_optional = is_option_type(param_type);
                let base_type = if is_optional {
                    extract_option_inner(param_type).unwrap_or(param_type)
                } else {
                    param_type
                };
                
                // Generate JSON schema property
                let schema_type = type_to_json_schema(base_type);
                schema_props.push(quote! {
                    props.insert(#param_name.to_string(), #schema_type);
                });
                
                if !is_optional {
                    required_params.push(param_name.clone());
                }
                
                // Generate parameter extraction code
                let extract = generate_param_extraction(&param_name, base_type, is_optional);
                param_extractions.push(extract);
            }
        }
    }
    
    let required_json = if required_params.is_empty() {
        quote! { serde_json::json!([]) }
    } else {
        quote! { serde_json::json!([#(#required_params),*]) }
    };
    
    // Generate the tool struct
    let tool_struct_name = format_ident!("{}", func_name.to_string().to_uppercase());
    let original_func_name = format_ident!("__{}_original", func_name);
    
    let expanded = quote! {
        // Original function with renamed identifier
        #[allow(non_snake_case)]
        fn #original_func_name(#(#func_params),*) #return_type {
            #func_body
        }
        
        // Generated tool registration
        pub static #tool_struct_name: std::sync::LazyLock<::velocity_mcp::registry::Tool> = std::sync::LazyLock::new(|| {
            let mut props = std::collections::HashMap::new();
            #(#schema_props)*
            
            ::velocity_mcp::registry::Tool {
                name: #tool_name.to_string(),
                description: #description.to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": props,
                    "required": #required_json
                }),
            }
        });
        
        // Generated dispatch function with original name
        pub fn #func_name(args: &serde_json::Value) -> Result<String, String> {
            #(#param_extractions)*
            #original_func_name(#(#param_names),*)
        }
    };
    
    expanded.into()
}

struct McpToolAttrs {
    name: Option<String>,
    description: Option<String>,
}

impl syn::parse::Parse for McpToolAttrs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut name = None;
        let mut description = None;
        
        while !input.is_empty() {
            let ident: syn::Ident = input.parse()?;
            let _eq: syn::Token![=] = input.parse()?;
            let value: LitStr = input.parse()?;
            
            match ident.to_string().as_str() {
                "name" => name = Some(value.value()),
                "description" => description = Some(value.value()),
                _ => return Err(syn::Error::new(ident.span(), "Unknown attribute")),
            }
            
            if !input.is_empty() {
                let _comma: syn::Token![,] = input.parse()?;
            }
        }
        
        Ok(McpToolAttrs { name, description })
    }
}

fn is_option_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            return segment.ident == "Option";
        }
    }
    false
}

fn extract_option_inner(ty: &Type) -> Option<&Type> {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            if segment.ident == "Option" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        return Some(inner);
                    }
                }
            }
        }
    }
    None
}

fn type_to_json_schema(ty: &Type) -> proc_macro2::TokenStream {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            let type_name = segment.ident.to_string();
            match type_name.as_str() {
                "String" | "str" => {
                    return quote! { serde_json::json!({"type": "string"}) };
                }
                "i8" | "i16" | "i32" | "i64" | "i128" | "isize" |
                "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => {
                    return quote! { serde_json::json!({"type": "integer"}) };
                }
                "f32" | "f64" => {
                    return quote! { serde_json::json!({"type": "number"}) };
                }
                "bool" => {
                    return quote! { serde_json::json!({"type": "boolean"}) };
                }
                "Vec" => {
                    return quote! { serde_json::json!({"type": "array"}) };
                }
                _ => {}
            }
        }
    }
    // Default to string for unknown types
    quote! { serde_json::json!({"type": "string"}) }
}

fn generate_param_extraction(param_name: &str, ty: &Type, is_optional: bool) -> proc_macro2::TokenStream {
    let param_ident = format_ident!("{}", param_name);
    
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            let type_name = segment.ident.to_string();
            
            let extract_expr = match type_name.as_str() {
                "String" => quote! {
                    args[#param_name].as_str().ok_or_else(|| format!("Missing required parameter: {}", #param_name))?.to_string()
                },
                "i64" | "i32" | "i16" | "i8" => quote! {
                    args[#param_name].as_i64().ok_or_else(|| format!("Missing required parameter: {}", #param_name))? as #ty
                },
                "u64" | "u32" | "u16" | "u8" => quote! {
                    args[#param_name].as_u64().ok_or_else(|| format!("Missing required parameter: {}", #param_name))? as #ty
                },
                "f64" | "f32" => quote! {
                    args[#param_name].as_f64().ok_or_else(|| format!("Missing required parameter: {}", #param_name))? as #ty
                },
                "bool" => quote! {
                    args[#param_name].as_bool().ok_or_else(|| format!("Missing required parameter: {}", #param_name))?
                },
                _ => quote! {
                    args[#param_name].as_str().ok_or_else(|| format!("Missing required parameter: {}", #param_name))?.to_string()
                },
            };
            
            if is_optional {
                return quote! {
                    let #param_ident: Option<#ty> = if args[#param_name].is_null() {
                        None
                    } else {
                        Some(#extract_expr)
                    };
                };
            } else {
                return quote! {
                    let #param_ident: #ty = #extract_expr;
                };
            }
        }
    }
    
    // Default extraction
    if is_optional {
        quote! {
            let #param_ident: Option<String> = args[#param_name].as_str().map(|s| s.to_string());
        }
    } else {
        quote! {
            let #param_ident: String = args[#param_name].as_str().ok_or_else(|| format!("Missing required parameter: {}", #param_name))?.to_string();
        }
    }
}
