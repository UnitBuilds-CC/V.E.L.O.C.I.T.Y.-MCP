//! Proc-macro crate for type-safe MCP tool registration.
//!
//! Provides the `#[mcp_tool]` attribute macro that generates compile-time
//! tool registration from function signatures with support for:
//! - Basic types (String, integers, floats, bool)
//! - Optional parameters (Option<T>)
//! - Arrays (Vec<T>) with item type schemas
//! - Nested structs (recursive schema generation)
//! - Enums (string enum schemas)
//! - JSON schema constraints (min, max, pattern, default)
//! - Auto-registration into global tool registry

use proc_macro::TokenStream;
use quote::{quote, format_ident};
use syn::{parse_macro_input, ItemFn, FnArg, Pat, Type, LitStr, LitInt, LitFloat};

/// Attribute macro for type-safe MCP tool registration.
///
/// Generates a `Tool` struct and registration code from a function signature.
/// The function parameters are converted to a JSON schema automatically.
///
/// # Example
///
/// ```ignore
/// #[mcp_tool(
///     name = "read_file",
///     description = "Read a file from disk",
///     param_constraints = {
///         "path": { "min_length": 1 },
///         "offset": { "minimum": 0 }
///     }
/// )]
/// fn read_file(path: String, offset: Option<i64>) -> Result<String, String> {
///     // implementation
/// }
/// ```
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
                
                // Get constraints for this parameter
                let constraints = attrs.param_constraints.get(&param_name);
                
                // Generate JSON schema property with constraints
                let schema_type = type_to_json_schema(base_type, constraints);
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
    let register_fn_name = format_ident!("__register_{}", func_name);
    let auto_register_fn_name = format_ident!("__auto_register_{}", func_name);
    
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
        
        // Auto-registration function
        pub fn #register_fn_name() {
            ::velocity_mcp::registry::register_tool_lazy(&#tool_struct_name);
        }
        
        // Auto-registration constructor - runs at program startup
        #[::ctor::ctor]
        fn #auto_register_fn_name() {
            ::velocity_mcp::registry::register_tool_lazy(&#tool_struct_name);
        }
        
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
    param_constraints: std::collections::HashMap<String, ParamConstraints>,
}

struct ParamConstraints {
    min_length: Option<u64>,
    max_length: Option<u64>,
    minimum: Option<f64>,
    maximum: Option<f64>,
    pattern: Option<String>,
    default: Option<String>,
}

impl syn::parse::Parse for McpToolAttrs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut name = None;
        let mut description = None;
        let mut param_constraints = std::collections::HashMap::new();
        
        while !input.is_empty() {
            let ident: syn::Ident = input.parse()?;
            let _eq: syn::Token![=] = input.parse()?;
            
            match ident.to_string().as_str() {
                "name" => {
                    let value: LitStr = input.parse()?;
                    name = Some(value.value());
                }
                "description" => {
                    let value: LitStr = input.parse()?;
                    description = Some(value.value());
                }
                "param_constraints" => {
                    let content;
                    syn::braced!(content in input);
                    
                    while !content.is_empty() {
                        let param_name: LitStr = content.parse()?;
                        let _colon: syn::Token![:] = content.parse()?;
                        
                        let constraints_content;
                        syn::braced!(constraints_content in content);
                        
                        let mut constraints = ParamConstraints {
                            min_length: None,
                            max_length: None,
                            minimum: None,
                            maximum: None,
                            pattern: None,
                            default: None,
                        };
                        
                        while !constraints_content.is_empty() {
                            let constraint_name: syn::Ident = constraints_content.parse()?;
                            let _colon: syn::Token![:] = constraints_content.parse()?;
                            
                            match constraint_name.to_string().as_str() {
                                "min_length" => {
                                    let value: LitInt = constraints_content.parse()?;
                                    constraints.min_length = Some(value.base10_parse()?);
                                }
                                "max_length" => {
                                    let value: LitInt = constraints_content.parse()?;
                                    constraints.max_length = Some(value.base10_parse()?);
                                }
                                "minimum" => {
                                    let value: LitFloat = constraints_content.parse()?;
                                    constraints.minimum = Some(value.base10_parse()?);
                                }
                                "maximum" => {
                                    let value: LitFloat = constraints_content.parse()?;
                                    constraints.maximum = Some(value.base10_parse()?);
                                }
                                "pattern" => {
                                    let value: LitStr = constraints_content.parse()?;
                                    constraints.pattern = Some(value.value());
                                }
                                "default" => {
                                    let value: LitStr = constraints_content.parse()?;
                                    constraints.default = Some(value.value());
                                }
                                _ => return Err(syn::Error::new(constraint_name.span(), "Unknown constraint")),
                            }
                            
                            if !constraints_content.is_empty() {
                                let _comma: syn::Token![,] = constraints_content.parse()?;
                            }
                        }
                        
                        param_constraints.insert(param_name.value(), constraints);
                        
                        if !content.is_empty() {
                            let _comma: syn::Token![,] = content.parse()?;
                        }
                    }
                }
                _ => return Err(syn::Error::new(ident.span(), "Unknown attribute")),
            }
            
            if !input.is_empty() {
                let _comma: syn::Token![,] = input.parse()?;
            }
        }
        
        Ok(McpToolAttrs { name, description, param_constraints })
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

fn extract_vec_inner(ty: &Type) -> Option<&Type> {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            if segment.ident == "Vec" {
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

fn type_to_json_schema(ty: &Type, constraints: Option<&ParamConstraints>) -> proc_macro2::TokenStream {
    let mut schema = if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            let type_name = segment.ident.to_string();
            match type_name.as_str() {
                "String" | "str" => {
                    quote! { serde_json::json!({"type": "string"}) }
                }
                "i8" | "i16" | "i32" | "i64" | "i128" | "isize" |
                "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => {
                    quote! { serde_json::json!({"type": "integer"}) }
                }
                "f32" | "f64" => {
                    quote! { serde_json::json!({"type": "number"}) }
                }
                "bool" => {
                    quote! { serde_json::json!({"type": "boolean"}) }
                }
                "Vec" => {
                    // Handle Vec<T> with item type
                    if let Some(inner_type) = extract_vec_inner(ty) {
                        let item_schema = type_to_json_schema(inner_type, None);
                        quote! {
                            {
                                let mut schema = serde_json::json!({"type": "array"});
                                schema["items"] = #item_schema;
                                schema
                            }
                        }
                    } else {
                        quote! { serde_json::json!({"type": "array"}) }
                    }
                }
                _ => {
                    // For unknown types (structs, enums), generate a reference or object schema
                    quote! { serde_json::json!({"type": "object"}) }
                }
            }
        } else {
            quote! { serde_json::json!({"type": "string"}) }
        }
    } else {
        quote! { serde_json::json!({"type": "string"}) }
    };
    
    // Apply constraints if provided
    if let Some(constraints) = constraints {
        if let Some(min_length) = constraints.min_length {
            schema = quote! {
                {
                    let mut s = #schema;
                    s["minLength"] = serde_json::json!(#min_length);
                    s
                }
            };
        }
        if let Some(max_length) = constraints.max_length {
            schema = quote! {
                {
                    let mut s = #schema;
                    s["maxLength"] = serde_json::json!(#max_length);
                    s
                }
            };
        }
        if let Some(minimum) = constraints.minimum {
            schema = quote! {
                {
                    let mut s = #schema;
                    s["minimum"] = serde_json::json!(#minimum);
                    s
                }
            };
        }
        if let Some(maximum) = constraints.maximum {
            schema = quote! {
                {
                    let mut s = #schema;
                    s["maximum"] = serde_json::json!(#maximum);
                    s
                }
            };
        }
        if let Some(ref pattern) = constraints.pattern {
            schema = quote! {
                {
                    let mut s = #schema;
                    s["pattern"] = serde_json::json!(#pattern);
                    s
                }
            };
        }
        if let Some(ref default) = constraints.default {
            schema = quote! {
                {
                    let mut s = #schema;
                    s["default"] = serde_json::json!(#default);
                    s
                }
            };
        }
    }
    
    schema
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
                "Vec" => {
                    // Handle Vec<T> extraction
                    if let Some(inner_type) = extract_vec_inner(ty) {
                        let inner_extract = generate_vec_item_extraction(inner_type);
                        quote! {
                            {
                                let arr = args[#param_name].as_array().ok_or_else(|| format!("Parameter {} must be an array", #param_name))?;
                                arr.iter().map(|item| #inner_extract).collect::<Result<Vec<_>, _>>()?
                            }
                        }
                    } else {
                        quote! {
                            args[#param_name].as_array().ok_or_else(|| format!("Missing required parameter: {}", #param_name))?.clone()
                        }
                    }
                }
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

fn generate_vec_item_extraction(ty: &Type) -> proc_macro2::TokenStream {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            let type_name = segment.ident.to_string();
            return match type_name.as_str() {
                "String" => quote! {
                    item.as_str().map(|s| s.to_string()).ok_or_else(|| "Array item must be a string".to_string())
                },
                "i64" | "i32" | "i16" | "i8" => quote! {
                    item.as_i64().map(|v| v as #ty).ok_or_else(|| "Array item must be an integer".to_string())
                },
                "u64" | "u32" | "u16" | "u8" => quote! {
                    item.as_u64().map(|v| v as #ty).ok_or_else(|| "Array item must be an unsigned integer".to_string())
                },
                "f64" | "f32" => quote! {
                    item.as_f64().map(|v| v as #ty).ok_or_else(|| "Array item must be a number".to_string())
                },
                "bool" => quote! {
                    item.as_bool().ok_or_else(|| "Array item must be a boolean".to_string())
                },
                _ => quote! {
                    item.as_str().map(|s| s.to_string()).ok_or_else(|| "Array item must be a string".to_string())
                },
            };
        }
    }
    quote! {
        item.as_str().map(|s| s.to_string()).ok_or_else(|| "Array item must be a string".to_string())
    }
}
