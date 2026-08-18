//! Native Rust implementation of the NDA (Neural Document Archive) binary format.
//!
//! Ported from the C# `Velocity.Core.NeuralDocument` reference implementation.
//! Provides zero-copy reading and compiling of NDA binary documents containing
//! semantic triples, visual display commands, and a deduplicated string pool.
//!
//! # Binary Layout
//!
//! ```text
//! [Header: 52 bytes]
//!   magic:            u32 LE  (0x3141444E = "NDA1")
//!   flags:            u32 LE
//!   merkle_root:      [u8; 32]
//!   triple_count:     u32 LE
//!   command_count:    u32 LE
//!   string_pool_off:  u32 LE
//! [SemanticTriples: triple_count × 12 bytes]
//!   subject_offset:   u32 LE  (string pool offset)
//!   predicate_offset: u32 LE
//!   object_offset:    u32 LE
//! [DisplayCommands: command_count × 17 bytes]
//!   command_type:     u8      (1=Text, 2=Vector, 3=Rect, 4=Image)
//!   color:            u32 LE  (RGBA)
//!   x, y, w, h:       u16 LE each
//!   content_offset:   u32 LE  (string pool offset)
//! [String Pool: remaining bytes]
//!   Per entry: u16 LE length + UTF-8 bytes
//!   Offset 0 = empty string
//! ```

use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::io::Cursor;

/// NDA magic number: 0x3141444E = "NDA1" in little-endian.
pub const NDA_MAGIC: u32 = 0x3141444E;

/// Size of the NDA header in bytes.
pub const HEADER_SIZE: usize = 52;

/// Size of a serialized semantic triple in bytes.
pub const TRIPLE_SIZE: usize = 12;

/// Size of a serialized display command in bytes.
pub const COMMAND_SIZE: usize = 17;

/// Maximum number of triples in a single NDA document (prevents OOM from malicious files).
pub const MAX_TRIPLES: usize = 1_000_000;

/// Maximum number of display commands in a single NDA document.
pub const MAX_COMMANDS: usize = 1_000_000;

/// Maximum string pool size (100 MB).
pub const MAX_STRING_POOL_SIZE: usize = 100_000_000;

/// A semantic triple (subject, predicate, object) with string pool offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticTriple {
    pub subject_offset: u32,
    pub predicate_offset: u32,
    pub object_offset: u32,
}

/// A visual display command with type, color, position, and content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayCommand {
    pub command_type: u8,
    pub color: u32,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub content_offset: u32,
}

/// A parsed NDA document with header, triples, commands, and string pool.
#[derive(Debug, Clone)]
pub struct NdaDocument {
    pub flags: u32,
    pub merkle_root: [u8; 32],
    pub triples: Vec<SemanticTriple>,
    pub commands: Vec<DisplayCommand>,
    string_pool: Vec<u8>,
}

impl NdaDocument {
    /// Parse an NDA document from raw bytes.
    ///
    /// Validates all bounds, counts, and offsets before allocating.
    /// Rejects malicious files with excessive counts or corrupted offsets.
    pub fn read(data: &[u8]) -> Result<Self, String> {
        if data.len() < HEADER_SIZE {
            return Err(format!("Buffer too small for NDA header: {} bytes", data.len()));
        }

        let mut cur = Cursor::new(data);
        use std::io::Read;

        let magic = read_u32_le(&mut cur)?;
        if magic != NDA_MAGIC {
            return Err(format!("Invalid NDA magic: 0x{:08X} (expected 0x{:08X})", magic, NDA_MAGIC));
        }

        let flags = read_u32_le(&mut cur)?;
        let mut merkle_root = [0u8; 32];
        cur.read_exact(&mut merkle_root).map_err(|e| format!("Failed to read merkle root: {}", e))?;
        let triple_count = read_u32_le(&mut cur)? as usize;
        let command_count = read_u32_le(&mut cur)? as usize;
        let string_pool_offset = read_u32_le(&mut cur)? as usize;

        // ── Bounds validation (overflow-safe) ──────────────────────────
        if triple_count > MAX_TRIPLES {
            return Err(format!("Triple count {} exceeds maximum {}", triple_count, MAX_TRIPLES));
        }
        if command_count > MAX_COMMANDS {
            return Err(format!("Command count {} exceeds maximum {}", command_count, MAX_COMMANDS));
        }

        let triples_size = triple_count.checked_mul(TRIPLE_SIZE)
            .ok_or("Integer overflow computing triples size")?;
        let commands_size = command_count.checked_mul(COMMAND_SIZE)
            .ok_or("Integer overflow computing commands size")?;
        let expected_min = HEADER_SIZE.checked_add(triples_size)
            .and_then(|v| v.checked_add(commands_size))
            .ok_or("Integer overflow computing expected size")?;

        if data.len() < expected_min {
            return Err(format!("NDA buffer corrupted: need {} bytes, have {}", expected_min, data.len()));
        }
        if string_pool_offset > data.len() {
            return Err(format!("String pool offset {} exceeds buffer size {}", string_pool_offset, data.len()));
        }
        if string_pool_offset < expected_min {
            return Err(format!("String pool offset {} overlaps with triples/commands (min {})", string_pool_offset, expected_min));
        }
        let string_pool_size = data.len() - string_pool_offset;
        if string_pool_size > MAX_STRING_POOL_SIZE {
            return Err(format!("String pool size {} exceeds maximum {}", string_pool_size, MAX_STRING_POOL_SIZE));
        }

        // Read triples
        let mut triples = Vec::with_capacity(triple_count);
        let triples_start = HEADER_SIZE;
        for i in 0..triple_count {
            let offset = triples_start + i * TRIPLE_SIZE;
            triples.push(SemanticTriple {
                subject_offset: u32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]]),
                predicate_offset: u32::from_le_bytes([data[offset+4], data[offset+5], data[offset+6], data[offset+7]]),
                object_offset: u32::from_le_bytes([data[offset+8], data[offset+9], data[offset+10], data[offset+11]]),
            });
        }

        // Read commands
        let mut commands = Vec::with_capacity(command_count);
        let commands_start = triples_start + triple_count * TRIPLE_SIZE;
        for i in 0..command_count {
            let offset = commands_start + i * COMMAND_SIZE;
            commands.push(DisplayCommand {
                command_type: data[offset],
                color: u32::from_le_bytes([data[offset+1], data[offset+2], data[offset+3], data[offset+4]]),
                x: u16::from_le_bytes([data[offset+5], data[offset+6]]),
                y: u16::from_le_bytes([data[offset+7], data[offset+8]]),
                width: u16::from_le_bytes([data[offset+9], data[offset+10]]),
                height: u16::from_le_bytes([data[offset+11], data[offset+12]]),
                content_offset: u32::from_le_bytes([data[offset+13], data[offset+14], data[offset+15], data[offset+16]]),
            });
        }

        // String pool
        let string_pool = data[string_pool_offset..].to_vec();

        // ── Validate string pool offsets for all triples ───────────────
        for (i, t) in triples.iter().enumerate() {
            for (name, off) in [("subject", t.subject_offset), ("predicate", t.predicate_offset), ("object", t.object_offset)] {
                if off != 0 {
                    let o = off as usize;
                    if o + 2 > string_pool.len() {
                        return Err(format!("Triple {} {} offset {} exceeds string pool size {}", i, name, o, string_pool.len()));
                    }
                    let slen = u16::from_le_bytes([string_pool[o], string_pool[o + 1]]) as usize;
                    if o + 2 + slen > string_pool.len() {
                        return Err(format!("Triple {} {} string at offset {} extends beyond pool", i, name, o));
                    }
                }
            }
        }

        // ── Validate display command types and content offsets ──────────
        for (i, c) in commands.iter().enumerate() {
            if c.command_type < 1 || c.command_type > 4 {
                return Err(format!("Command {} has invalid type {} (expected 1-4)", i, c.command_type));
            }
            if c.content_offset != 0 {
                let o = c.content_offset as usize;
                if o + 2 > string_pool.len() {
                    return Err(format!("Command {} content offset {} exceeds string pool size {}", i, o, string_pool.len()));
                }
                let slen = u16::from_le_bytes([string_pool[o], string_pool[o + 1]]) as usize;
                if o + 2 + slen > string_pool.len() {
                    return Err(format!("Command {} content string at offset {} extends beyond pool", i, o));
                }
            }
        }

        Ok(NdaDocument {
            flags,
            merkle_root,
            triples,
            commands,
            string_pool,
        })
    }

    /// Resolve a string pool offset to its UTF-8 string.
    /// Offset 0 returns an empty string.
    pub fn get_string(&self, offset: u32) -> Result<String, String> {
        if offset == 0 {
            return Ok(String::new());
        }
        let off = offset as usize;
        if off + 2 > self.string_pool.len() {
            return Err(format!("String offset {} exceeds string pool size {}", off, self.string_pool.len()));
        }
        let len = u16::from_le_bytes([self.string_pool[off], self.string_pool[off + 1]]) as usize;
        if off + 2 + len > self.string_pool.len() {
            return Err(format!("String at offset {} extends beyond string pool", off));
        }
        String::from_utf8(self.string_pool[off + 2..off + 2 + len].to_vec())
            .map_err(|e| format!("Invalid UTF-8 in string pool at offset {}: {}", off, e))
    }

    /// Format the document as a human-readable inspection report.
    /// Matches the C# `read_nda` output format exactly.
    pub fn format_inspection(&self, filename: &str) -> Result<String, String> {
        let mut out = String::new();
        out.push_str(&format!("=== NDA Document Inspection: {} ===\n", filename));
        out.push_str(&format!("Merkle Root Signature: {}\n", hex_encode(&self.merkle_root)));
        out.push_str(&format!("Triples Count: {}\n", self.triples.len()));
        out.push_str(&format!("Display Commands Count: {}\n", self.commands.len()));
        out.push_str("\n--- Semantic Triples ---\n");

        for t in &self.triples {
            let s = self.get_string(t.subject_offset)?;
            let p = self.get_string(t.predicate_offset)?;
            let o = self.get_string(t.object_offset)?;
            out.push_str(&format!("({}, {}, {})\n", s, p, o));
        }

        out.push_str("\n--- Visual Display Commands ---\n");
        for c in &self.commands {
            let content = self.get_string(c.content_offset)?;
            out.push_str(&format!(
                "Command Type: {} | Color: {:08X} | X: {} | Y: {} | Width: {} | Height: {} | Content: {}\n",
                c.command_type, c.color, c.x, c.y, c.width, c.height, content
            ));
        }

        Ok(out)
    }

    /// Verify the document's Merkle root integrity.
    ///
    /// Recomputes the Merkle root from the parsed triples and compares it
    /// against the stored root in the header. Returns `Ok(())` if they match,
    /// or an error describing the mismatch.
    pub fn verify_merkle(&self) -> Result<(), String> {
        let computed = self.recompute_merkle_root()?;
        if computed == self.merkle_root {
            Ok(())
        } else {
            Err(format!(
                "Merkle root mismatch: stored={} computed={}",
                hex_encode(&self.merkle_root),
                hex_encode(&computed)
            ))
        }
    }

    /// Recompute the Merkle root from the parsed triples.
    /// Each leaf = SHA-256("S|P|O"). Pair-wise hash up; odd leaves promoted.
    fn recompute_merkle_root(&self) -> Result<[u8; 32], String> {
        use sha2::{Sha256, Digest};

        if self.triples.is_empty() {
            return Ok([0u8; 32]);
        }

        let mut leaves: Vec<[u8; 32]> = Vec::with_capacity(self.triples.len());
        for t in &self.triples {
            let s = self.get_string(t.subject_offset)?;
            let p = self.get_string(t.predicate_offset)?;
            let o = self.get_string(t.object_offset)?;
            let repr = format!("{}|{}|{}", s, p, o);
            let mut h = Sha256::new();
            h.update(repr.as_bytes());
            let result = h.finalize();
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&result);
            leaves.push(arr);
        }

        let mut current_level = leaves;
        while current_level.len() > 1 {
            let mut next_level = Vec::new();
            let mut i = 0;
            while i < current_level.len() {
                if i + 1 < current_level.len() {
                    let mut combined = [0u8; 64];
                    combined[..32].copy_from_slice(&current_level[i]);
                    combined[32..].copy_from_slice(&current_level[i + 1]);
                    let mut h = Sha256::new();
                    h.update(combined);
                    let result = h.finalize();
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&result);
                    next_level.push(arr);
                    i += 2;
                } else {
                    next_level.push(current_level[i]);
                    i += 1;
                }
            }
            current_level = next_level;
        }

        Ok(current_level[0])
    }
}

// ─── Compiler ─────────────────────────────────────────────────────────────────

/// Compiles semantic triples and display commands into NDA binary format.
///
/// Mirrors the C# `NeuralDocument.Compiler` class. Strings are deduplicated
/// into a string pool, and a Merkle root is computed from the triple leaves.
pub struct NdaCompiler {
    triples: Vec<(String, String, String)>,
    commands: Vec<CommandInfo>,
    string_pool: HashMap<String, u32>,
    string_pool_data: Vec<u8>,
}

/// Input command before compilation (content is still a string, not an offset).
pub struct CommandInfo {
    pub command_type: u8,
    pub color: u32,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub content: String,
}

impl Default for NdaCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl NdaCompiler {
    pub fn new() -> Self {
        let mut string_pool = HashMap::new();
        let mut string_pool_data = Vec::new();
        // Reserve offset 0 for empty string: write 2 zero bytes (length=0 placeholder)
        // Real strings start at offset 2+
        string_pool.insert(String::new(), 0);
        string_pool_data.extend_from_slice(&0u16.to_le_bytes());
        NdaCompiler {
            triples: Vec::new(),
            commands: Vec::new(),
            string_pool,
            string_pool_data,
        }
    }

    /// Add a semantic triple (subject, predicate, object).
    pub fn add_triple(&mut self, subject: &str, predicate: &str, object: &str) {
        self.triples.push((subject.to_string(), predicate.to_string(), object.to_string()));
    }

    /// Add a visual display command.
    #[allow(clippy::too_many_arguments)]
    pub fn add_command(&mut self, command_type: u8, color: u32, x: u16, y: u16, w: u16, h: u16, content: &str) {
        self.commands.push(CommandInfo {
            command_type,
            color,
            x, y,
            width: w,
            height: h,
            content: content.to_string(),
        });
    }

    /// Compile all triples and commands into an NDA binary document.
    pub fn compile(mut self) -> Vec<u8> {
        let merkle_root = self.compute_merkle_root();

        // Register all strings and build compiled triples
        // (collect into temp vecs first to avoid borrow checker conflicts)
        let triple_strings: Vec<(String, String, String)> = self.triples.clone();
        let compiled_triples: Vec<SemanticTriple> = triple_strings.iter().map(|(s, p, o)| {
            SemanticTriple {
                subject_offset: self.get_or_add_string(s),
                predicate_offset: self.get_or_add_string(p),
                object_offset: self.get_or_add_string(o),
            }
        }).collect();

        // Register command strings and build compiled commands
        let cmd_infos: Vec<(u8, u32, u16, u16, u16, u16, String)> = self.commands.iter().map(|c| {
            (c.command_type, c.color, c.x, c.y, c.width, c.height, c.content.clone())
        }).collect();
        let compiled_commands: Vec<DisplayCommand> = cmd_infos.iter().map(|(ct, color, x, y, w, h, content)| {
            DisplayCommand {
                command_type: *ct,
                color: *color,
                x: *x,
                y: *y,
                width: *w,
                height: *h,
                content_offset: self.get_or_add_string(content),
            }
        }).collect();

        let string_pool_offset = HEADER_SIZE
            + compiled_triples.len() * TRIPLE_SIZE
            + compiled_commands.len() * COMMAND_SIZE;

        let total_size = string_pool_offset + self.string_pool_data.len();
        let mut buffer = Vec::with_capacity(total_size);

        // Write header
        buffer.extend_from_slice(&NDA_MAGIC.to_le_bytes());
        buffer.extend_from_slice(&0u32.to_le_bytes()); // flags
        buffer.extend_from_slice(&merkle_root);
        buffer.extend_from_slice(&(compiled_triples.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&(compiled_commands.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&(string_pool_offset as u32).to_le_bytes());

        // Write triples
        for t in &compiled_triples {
            buffer.extend_from_slice(&t.subject_offset.to_le_bytes());
            buffer.extend_from_slice(&t.predicate_offset.to_le_bytes());
            buffer.extend_from_slice(&t.object_offset.to_le_bytes());
        }

        // Write commands
        for c in &compiled_commands {
            buffer.push(c.command_type);
            buffer.extend_from_slice(&c.color.to_le_bytes());
            buffer.extend_from_slice(&c.x.to_le_bytes());
            buffer.extend_from_slice(&c.y.to_le_bytes());
            buffer.extend_from_slice(&c.width.to_le_bytes());
            buffer.extend_from_slice(&c.height.to_le_bytes());
            buffer.extend_from_slice(&c.content_offset.to_le_bytes());
        }

        // Write string pool
        buffer.extend_from_slice(&self.string_pool_data);

        buffer
    }

    /// Get or insert a string into the pool, returning its offset.
    fn get_or_add_string(&mut self, s: &str) -> u32 {
        if let Some(&offset) = self.string_pool.get(s) {
            return offset;
        }
        let offset = self.string_pool_data.len() as u32;
        let bytes = s.as_bytes();
        self.string_pool_data.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        self.string_pool_data.extend_from_slice(bytes);
        self.string_pool.insert(s.to_string(), offset);
        offset
    }

    /// Compute the Merkle root from all triples.
    /// Each leaf = SHA-256("S|P|O"). Pair-wise hash up; odd leaves promoted.
    fn compute_merkle_root(&self) -> [u8; 32] {
        if self.triples.is_empty() {
            return [0u8; 32];
        }

        let leaves: Vec<[u8; 32]> = self.triples.iter().map(|(s, p, o)| {
            let repr = format!("{}|{}|{}", s, p, o);
            let mut h = Sha256::new();
            h.update(repr.as_bytes());
            let result = h.finalize();
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&result);
            arr
        }).collect();

        let mut current_level = leaves;
        while current_level.len() > 1 {
            let mut next_level = Vec::new();
            let mut i = 0;
            while i < current_level.len() {
                if i + 1 < current_level.len() {
                    let mut combined = [0u8; 64];
                    combined[..32].copy_from_slice(&current_level[i]);
                    combined[32..].copy_from_slice(&current_level[i + 1]);
                    let mut h = Sha256::new();
                    h.update(combined);
                    let result = h.finalize();
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&result);
                    next_level.push(arr);
                    i += 2;
                } else {
                    next_level.push(current_level[i]);
                    i += 1;
                }
            }
            current_level = next_level;
        }

        current_level[0]
    }
}

// ─── Ed25519 Signature Support ───────────────────────────────────────────────

/// Size of the Ed25519 signature section appended after the string pool.
/// Format: signature_len (u32 LE) + signature (64 bytes) + public_key (32 bytes)
pub const SIGNATURE_SECTION_SIZE: usize = 4 + 64 + 32; // 100 bytes

/// Ed25519 signature section appended to signed NDA documents.
#[derive(Debug, Clone)]
pub struct NdaSignature {
    /// The 64-byte Ed25519 signature.
    pub signature: [u8; 64],
    /// The 32-byte Ed25519 public key of the signer.
    pub public_key: [u8; 32],
}

impl NdaDocument {
    /// Check if this document has a signature section.
    pub fn has_signature(data: &[u8]) -> bool {
        // After the string pool, there must be exactly SIGNATURE_SECTION_SIZE bytes
        // We check if the data is large enough to contain a signature after the pool
        if data.len() < HEADER_SIZE + SIGNATURE_SECTION_SIZE {
            return false;
        }
        // Read the signature_len at the expected position
        // We need to know where the string pool ends, which requires parsing the header
        if let Ok(_doc) = NdaDocument::read(data) {
            // The string pool starts at string_pool_offset and extends to end of pool data
            // If there's a signature section, data.len() > string_pool_offset + pool_len + SIGNATURE_SECTION_SIZE
            // For simplicity, check if the last SIGNATURE_SECTION_SIZE bytes look like a signature
            let sig_start = data.len().saturating_sub(SIGNATURE_SECTION_SIZE);
            let sig_len = u32::from_le_bytes([
                data[sig_start], data[sig_start+1], data[sig_start+2], data[sig_start+3]
            ]);
            sig_len == 64
        } else {
            false
        }
    }

    /// Read the signature section from raw NDA bytes (if present).
    pub fn read_signature(data: &[u8]) -> Result<Option<NdaSignature>, String> {
        if !Self::has_signature(data) {
            return Ok(None);
        }
        let sig_start = data.len() - SIGNATURE_SECTION_SIZE;
        let sig_len = u32::from_le_bytes([
            data[sig_start], data[sig_start+1], data[sig_start+2], data[sig_start+3]
        ]);
        if sig_len != 64 {
            return Err("Invalid signature section length".to_string());
        }

        let mut signature = [0u8; 64];
        signature.copy_from_slice(&data[sig_start + 4..sig_start + 68]);

        let mut public_key = [0u8; 32];
        public_key.copy_from_slice(&data[sig_start + 68..sig_start + 100]);

        Ok(Some(NdaSignature { signature, public_key }))
    }

    /// Verify the Ed25519 signature of this document.
    ///
    /// The signature covers all bytes from the start of the NDA up to (but not
    /// including) the signature section. Returns Ok(()) if the signature is valid.
    pub fn verify_signature(data: &[u8]) -> Result<(), String> {
        use ed25519_dalek::{Verifier, VerifyingKey, Signature};

        let nda_sig = Self::read_signature(data)?
            .ok_or("Document is not signed")?;

        let signed_data_len = data.len() - SIGNATURE_SECTION_SIZE;
        let signed_data = &data[..signed_data_len];

        let verifying_key = VerifyingKey::from_bytes(&nda_sig.public_key)
            .map_err(|e| format!("Invalid public key: {}", e))?;

        let signature = Signature::from_bytes(&nda_sig.signature);

        verifying_key.verify(signed_data, &signature)
            .map_err(|e| format!("Signature verification failed: {}", e))
    }
}

impl NdaCompiler {
    /// Compile the NDA document and sign it with the given Ed25519 keypair.
    ///
    /// Appends a signature section after the string pool. The signature covers
    /// the entire unsigned NDA binary.
    pub fn compile_signed(
        self,
        signing_key: &ed25519_dalek::SigningKey,
    ) -> Vec<u8> {
        use ed25519_dalek::Signer;

        let mut data = self.compile();

        // Sign the entire unsigned NDA binary
        let signature = signing_key.sign(&data);
        let public_key = signing_key.verifying_key();

        // Append signature section
        data.extend_from_slice(&64u32.to_le_bytes()); // signature_len = 64
        data.extend_from_slice(&signature.to_bytes());
        data.extend_from_slice(public_key.as_bytes());

        data
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn read_u32_le(cur: &mut Cursor<&[u8]>) -> Result<u32, String> {
    use std::io::Read;
    let mut buf = [0u8; 4];
    cur.read_exact(&mut buf).map_err(|e| format!("Failed to read u32: {}", e))?;
    Ok(u32::from_le_bytes(buf))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02X}", b)).collect()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_empty_document() {
        let compiler = NdaCompiler::new();
        let data = compiler.compile();
        // Header (52) + string pool placeholder (2 bytes for empty string at offset 0)
        assert_eq!(data.len(), HEADER_SIZE + 2);
        let doc = NdaDocument::read(&data).unwrap();
        assert_eq!(doc.triples.len(), 0);
        assert_eq!(doc.commands.len(), 0);
        assert_eq!(doc.merkle_root, [0u8; 32]); // empty merkle
    }

    #[test]
    fn test_compile_and_read_round_trip() {
        let mut compiler = NdaCompiler::new();
        compiler.add_triple("subject1", "PREDICATE", "object1");
        compiler.add_triple("subject2", "TYPE", "TestDoc");
        compiler.add_command(1, 0xFFFFFFFF, 10, 20, 100, 50, "Hello World");
        compiler.add_command(3, 0x00E5FFFF, 0, 0, 640, 480, "");

        let data = compiler.compile();
        let doc = NdaDocument::read(&data).unwrap();

        assert_eq!(doc.triples.len(), 2);
        assert_eq!(doc.commands.len(), 2);

        // Verify triples via string resolution
        assert_eq!(doc.get_string(doc.triples[0].subject_offset).unwrap(), "subject1");
        assert_eq!(doc.get_string(doc.triples[0].predicate_offset).unwrap(), "PREDICATE");
        assert_eq!(doc.get_string(doc.triples[0].object_offset).unwrap(), "object1");
        assert_eq!(doc.get_string(doc.triples[1].subject_offset).unwrap(), "subject2");
        assert_eq!(doc.get_string(doc.triples[1].predicate_offset).unwrap(), "TYPE");
        assert_eq!(doc.get_string(doc.triples[1].object_offset).unwrap(), "TestDoc");

        // Verify commands
        assert_eq!(doc.commands[0].command_type, 1);
        assert_eq!(doc.commands[0].color, 0xFFFFFFFF);
        assert_eq!(doc.commands[0].x, 10);
        assert_eq!(doc.commands[0].y, 20);
        assert_eq!(doc.get_string(doc.commands[0].content_offset).unwrap(), "Hello World");

        assert_eq!(doc.commands[1].command_type, 3);
        assert_eq!(doc.get_string(doc.commands[1].content_offset).unwrap(), "");
    }

    #[test]
    fn test_string_pool_deduplication() {
        let mut compiler = NdaCompiler::new();
        compiler.add_triple("shared", "PRED", "value");
        compiler.add_triple("shared", "OTHER", "value"); // "shared" and "value" reused

        let data = compiler.compile();
        let doc = NdaDocument::read(&data).unwrap();

        // Both triples should resolve to the same strings
        assert_eq!(doc.get_string(doc.triples[0].subject_offset).unwrap(), "shared");
        assert_eq!(doc.get_string(doc.triples[1].subject_offset).unwrap(), "shared");
        assert_eq!(doc.get_string(doc.triples[0].object_offset).unwrap(), "value");
        assert_eq!(doc.get_string(doc.triples[1].object_offset).unwrap(), "value");

        // Subject offsets should be identical (deduplicated)
        assert_eq!(doc.triples[0].subject_offset, doc.triples[1].subject_offset);
        assert_eq!(doc.triples[0].object_offset, doc.triples[1].object_offset);
    }

    #[test]
    fn test_merkle_root_nonzero() {
        let mut compiler = NdaCompiler::new();
        compiler.add_triple("a", "b", "c");
        let data = compiler.compile();
        let doc = NdaDocument::read(&data).unwrap();
        assert_ne!(doc.merkle_root, [0u8; 32]);
    }

    #[test]
    fn test_merkle_root_matches_csharp() {
        // Single triple: SHA-256("a|b|c") should be the leaf and root
        let mut compiler = NdaCompiler::new();
        compiler.add_triple("a", "b", "c");
        let data = compiler.compile();
        let doc = NdaDocument::read(&data).unwrap();

        let mut h = Sha256::new();
        h.update(b"a|b|c");
        let expected = h.finalize();
        assert_eq!(&doc.merkle_root[..], expected.as_slice());
    }

    #[test]
    fn test_invalid_magic_rejected() {
        let mut data = vec![0u8; HEADER_SIZE];
        // Write wrong magic
        data[0..4].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        assert!(NdaDocument::read(&data).is_err());
    }

    #[test]
    fn test_too_small_buffer_rejected() {
        let data = vec![0u8; 10];
        assert!(NdaDocument::read(&data).is_err());
    }

    #[test]
    fn test_format_inspection_output() {
        let mut compiler = NdaCompiler::new();
        compiler.add_triple("DOC_1", "TYPE", "TestDocument");
        compiler.add_command(1, 0xFFFFFFFF, 40, 40, 400, 25, "Title");

        let data = compiler.compile();
        let doc = NdaDocument::read(&data).unwrap();
        let report = doc.format_inspection("test.nda").unwrap();

        assert!(report.contains("=== NDA Document Inspection: test.nda ==="));
        assert!(report.contains("Merkle Root Signature:"));
        assert!(report.contains("Triples Count: 1"));
        assert!(report.contains("Display Commands Count: 1"));
        assert!(report.contains("(DOC_1, TYPE, TestDocument)"));
        assert!(report.contains("Command Type: 1"));
        assert!(report.contains("Content: Title"));
    }

    #[test]
    fn test_empty_string_at_offset_zero() {
        let compiler = NdaCompiler::new();
        let data = compiler.compile();
        let doc = NdaDocument::read(&data).unwrap();
        assert_eq!(doc.get_string(0).unwrap(), "");
    }

    // ── Adversarial / Security Tests ─────────────────────────────────────

    /// Craft a binary with huge triple_count → should be rejected before allocation.
    #[test]
    fn test_reject_excessive_triple_count() {
        let mut data = vec![0u8; HEADER_SIZE];
        data[0..4].copy_from_slice(&NDA_MAGIC.to_le_bytes());
        // Set triple_count to u32::MAX at offset 40
        data[40..44].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        let result = NdaDocument::read(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exceeds maximum"));
    }

    /// Craft a binary with huge command_count → should be rejected.
    #[test]
    fn test_reject_excessive_command_count() {
        let mut data = vec![0u8; HEADER_SIZE];
        data[0..4].copy_from_slice(&NDA_MAGIC.to_le_bytes());
        // Set command_count to u32::MAX at offset 44
        data[44..48].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        let result = NdaDocument::read(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exceeds maximum"));
    }

    /// Craft a binary with string_pool_offset overlapping triples/commands.
    #[test]
    fn test_reject_overlapping_string_pool() {
        let mut data = vec![0u8; HEADER_SIZE + 100];
        data[0..4].copy_from_slice(&NDA_MAGIC.to_le_bytes());
        // 1 triple (12 bytes), string_pool_offset = HEADER_SIZE (overlaps triple area)
        data[40..44].copy_from_slice(&1u32.to_le_bytes()); // triple_count = 1
        data[48..52].copy_from_slice(&(HEADER_SIZE as u32).to_le_bytes()); // string_pool_offset = 52 (overlaps)
        let result = NdaDocument::read(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("overlaps"));
    }

    /// Craft a binary with invalid command type (0 or 5).
    #[test]
    fn test_reject_invalid_command_type() {
        let mut compiler = NdaCompiler::new();
        compiler.add_triple("s", "p", "o");
        compiler.add_command(1, 0xFFFFFF, 0, 0, 100, 20, "test");
        let mut data = compiler.compile();
        // Find the command area (after header + 1 triple) and set type to 0
        let cmd_start = HEADER_SIZE + 1 * TRIPLE_SIZE;
        data[cmd_start] = 0; // Invalid type
        let result = NdaDocument::read(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid type"));
    }

    /// Craft a binary with a triple whose string offset points beyond the pool.
    #[test]
    fn test_reject_triple_beyond_string_pool() {
        let mut compiler = NdaCompiler::new();
        compiler.add_triple("s", "p", "o");
        let mut data = compiler.compile();
        // Corrupt the first triple's subject_offset to point beyond the string pool
        let triple_start = HEADER_SIZE;
        let bad_offset = 0xFFFF_FFFFu32;
        data[triple_start..triple_start+4].copy_from_slice(&bad_offset.to_le_bytes());
        let result = NdaDocument::read(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exceeds string pool"));
    }

    /// Random bytes should not parse as valid NDA.
    #[test]
    fn test_reject_random_bytes() {
        let data: Vec<u8> = (0..200).map(|i| (i * 37 + 13) as u8).collect();
        let result = NdaDocument::read(&data);
        assert!(result.is_err());
    }

    // ── Ed25519 Signature Tests ──────────────────────────────────────────

    #[test]
    fn test_sign_and_verify_round_trip() {
        use ed25519_dalek::SigningKey;
        let signing_key = SigningKey::from_bytes(&[42u8; 32]);

        let mut compiler = NdaCompiler::new();
        compiler.add_triple("DOC_1", "TYPE", "SignedDoc");
        compiler.add_command(1, 0xFFFFFFFF, 10, 20, 100, 50, "Signed Content");

        // Compile unsigned version of the same document
        let mut unsigned_compiler = NdaCompiler::new();
        unsigned_compiler.add_triple("DOC_1", "TYPE", "SignedDoc");
        unsigned_compiler.add_command(1, 0xFFFFFFFF, 10, 20, 100, 50, "Signed Content");
        let unsigned_data = unsigned_compiler.compile();

        let signed_data = compiler.compile_signed(&signing_key);

        // Signed should be exactly SIGNATURE_SECTION_SIZE bytes larger
        assert_eq!(signed_data.len() - unsigned_data.len(), SIGNATURE_SECTION_SIZE);

        // Verify signature
        assert!(NdaDocument::has_signature(&signed_data));
        let result = NdaDocument::verify_signature(&signed_data);
        assert!(result.is_ok(), "Signature should verify: {:?}", result);
    }

    #[test]
    fn test_unsigned_document_not_detected_as_signed() {
        let mut compiler = NdaCompiler::new();
        compiler.add_triple("DOC_1", "TYPE", "UnsignedDoc");
        let data = compiler.compile();

        assert!(!NdaDocument::has_signature(&data));
        let result = NdaDocument::verify_signature(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_tampered_signed_document_fails_verification() {
        use ed25519_dalek::SigningKey;
        let signing_key = SigningKey::from_bytes(&[42u8; 32]);

        let mut compiler = NdaCompiler::new();
        compiler.add_triple("DOC_1", "TYPE", "TamperedDoc");
        let mut signed_data = compiler.compile_signed(&signing_key);

        // Tamper with the document (flip a byte in the header area)
        signed_data[10] ^= 0xFF;

        let result = NdaDocument::verify_signature(&signed_data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("verification failed"));
    }

    #[test]
    fn test_wrong_key_fails_verification() {
        use ed25519_dalek::SigningKey;
        let signing_key = SigningKey::from_bytes(&[42u8; 32]);
        let wrong_key = SigningKey::from_bytes(&[99u8; 32]);

        let mut compiler = NdaCompiler::new();
        compiler.add_triple("DOC_1", "TYPE", "TestDoc");
        let signed_data = compiler.compile_signed(&signing_key);

        // Try to verify with a different key's public key
        let mut tampered = signed_data.clone();
        let sig_start = tampered.len() - SIGNATURE_SECTION_SIZE;
        // Replace public key with the wrong key's public key
        tampered[sig_start + 68..sig_start + 100].copy_from_slice(wrong_key.verifying_key().as_bytes());

        let result = NdaDocument::verify_signature(&tampered);
        assert!(result.is_err());
    }

    #[test]
    fn test_signed_document_still_parses() {
        use ed25519_dalek::SigningKey;
        let signing_key = SigningKey::from_bytes(&[42u8; 32]);

        let mut compiler = NdaCompiler::new();
        compiler.add_triple("DOC_1", "TYPE", "ParseTest");
        compiler.add_command(1, 0xFFFFFFFF, 10, 20, 100, 50, "Content");
        let signed_data = compiler.compile_signed(&signing_key);

        // The document should still parse correctly
        let doc = NdaDocument::read(&signed_data).unwrap();
        assert_eq!(doc.triples.len(), 1);
        assert_eq!(doc.commands.len(), 1);
        assert_eq!(doc.get_string(doc.triples[0].subject_offset).unwrap(), "DOC_1");
    }

    // ── Comprehensive Fuzz / Adversarial Tests ───────────────────────────

    /// Test that truncating a signed document at various points is handled.
    #[test]
    fn test_truncated_signed_document() {
        use ed25519_dalek::SigningKey;
        let signing_key = SigningKey::from_bytes(&[42u8; 32]);

        let mut compiler = NdaCompiler::new();
        compiler.add_triple("DOC_1", "TYPE", "TestDoc");
        let signed_data = compiler.compile_signed(&signing_key);

        // Truncate at various points
        for truncate_at in [10, 52, 100, signed_data.len() - 10] {
            if truncate_at >= signed_data.len() { continue; }
            let truncated = &signed_data[..truncate_at];
            // Should either fail to parse or fail signature verification
            if let Ok(_) = NdaDocument::read(truncated) {
                assert!(!NdaDocument::has_signature(truncated));
            }
        }
    }

    /// Test that corrupting signature bytes is detected.
    #[test]
    fn test_corrupted_signature_bytes() {
        use ed25519_dalek::SigningKey;
        let signing_key = SigningKey::from_bytes(&[42u8; 32]);

        let mut compiler = NdaCompiler::new();
        compiler.add_triple("s", "p", "o");
        let signed_data = compiler.compile_signed(&signing_key);

        // Corrupt each byte of the signature section
        let sig_start = signed_data.len() - SIGNATURE_SECTION_SIZE;
        for i in sig_start..signed_data.len() {
            let mut tampered = signed_data.clone();
            tampered[i] ^= 0x01;
            // Either parsing fails or verification fails
            if NdaDocument::has_signature(&tampered) {
                assert!(NdaDocument::verify_signature(&tampered).is_err());
            }
        }
    }

    /// Test header with all flags set.
    #[test]
    fn test_all_flags_set() {
        let mut compiler = NdaCompiler::new();
        compiler.add_triple("s", "p", "o");
        let mut data = compiler.compile();
        // Set all flag bits
        data[4..8].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        // Should still parse (flags are informational)
        let doc = NdaDocument::read(&data).unwrap();
        assert_eq!(doc.flags, 0xFFFF_FFFF);
    }

    /// Test with maximum valid string pool offset.
    #[test]
    fn test_string_pool_at_exact_end() {
        let mut compiler = NdaCompiler::new();
        compiler.add_triple("s", "p", "o");
        let data = compiler.compile();
        // This is the normal case — string pool at exact end of triples/commands
        let doc = NdaDocument::read(&data).unwrap();
        assert_eq!(doc.triples.len(), 1);
    }

    /// Test that a document with zero-length strings works correctly.
    #[test]
    fn test_zero_length_strings() {
        let mut compiler = NdaCompiler::new();
        compiler.add_triple("", "", "");
        let data = compiler.compile();
        let doc = NdaDocument::read(&data).unwrap();
        assert_eq!(doc.get_string(doc.triples[0].subject_offset).unwrap(), "");
        assert_eq!(doc.get_string(doc.triples[0].predicate_offset).unwrap(), "");
        assert_eq!(doc.get_string(doc.triples[0].object_offset).unwrap(), "");
    }

    /// Test Merkle verification on a signed document.
    #[test]
    fn test_merkle_verification_on_signed_document() {
        use ed25519_dalek::SigningKey;
        let signing_key = SigningKey::from_bytes(&[42u8; 32]);

        let mut compiler = NdaCompiler::new();
        compiler.add_triple("a", "b", "c");
        let signed_data = compiler.compile_signed(&signing_key);

        let doc = NdaDocument::read(&signed_data).unwrap();
        assert!(doc.verify_merkle().is_ok());
    }
}
