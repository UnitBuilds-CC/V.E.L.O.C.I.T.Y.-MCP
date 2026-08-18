//! Property-based fuzz tests using proptest.
//!
//! These tests generate random inputs to verify that the NDA parser, compiler,
//! and signature verification never panic and always produce correct results.

use proptest::prelude::*;
use velocity_mcp::nda_document::{NdaCompiler, NdaDocument, NDA_MAGIC, HEADER_SIZE, SIGNATURE_SECTION_SIZE};

// ─── NDA Compiler/Parser Round-Trip ──────────────────────────────────────────

/// Property: any document compiled from arbitrary triples survives a
/// compile → read round-trip with all data preserved exactly.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn prop_nda_round_trip_preserves_triples(
        subjects in proptest::collection::vec(any::<String>(), 1..10),
        predicates in proptest::collection::vec(any::<String>(), 1..10),
        objects in proptest::collection::vec(any::<String>(), 1..10),
    ) {
        let mut compiler = NdaCompiler::new();
        let count = subjects.len().min(predicates.len()).min(objects.len());
        for i in 0..count {
            compiler.add_triple(&subjects[i], &predicates[i], &objects[i]);
        }
        let data = compiler.compile();

        let doc = NdaDocument::read(&data).expect("Compiled NDA must parse");
        prop_assert_eq!(doc.triples.len(), count);

        for (i, triple) in doc.triples.iter().enumerate() {
            let s = doc.get_string(triple.subject_offset).unwrap();
            let p = doc.get_string(triple.predicate_offset).unwrap();
            let o = doc.get_string(triple.object_offset).unwrap();
            prop_assert_eq!(&s, &subjects[i]);
            prop_assert_eq!(&p, &predicates[i]);
            prop_assert_eq!(&o, &objects[i]);
        }
    }

    #[test]
    fn prop_nda_round_trip_preserves_commands(
        cmd_types in proptest::collection::vec(1u8..=4, 1..10),
        colors in proptest::collection::vec(any::<u32>(), 1..10),
        xs in proptest::collection::vec(any::<u16>(), 1..10),
        ys in proptest::collection::vec(any::<u16>(), 1..10),
        ws in proptest::collection::vec(any::<u16>(), 1..10),
        hs in proptest::collection::vec(any::<u16>(), 1..10),
        contents in proptest::collection::vec(any::<String>(), 1..10),
    ) {
        let mut compiler = NdaCompiler::new();
        let count = cmd_types.len().min(colors.len()).min(xs.len()).min(ys.len())
            .min(ws.len()).min(hs.len()).min(contents.len());
        for i in 0..count {
            compiler.add_command(cmd_types[i], colors[i], xs[i], ys[i], ws[i], hs[i], &contents[i]);
        }
        let data = compiler.compile();

        let doc = NdaDocument::read(&data).expect("Compiled NDA must parse");
        prop_assert_eq!(doc.commands.len(), count);

        for (i, cmd) in doc.commands.iter().enumerate() {
            prop_assert_eq!(cmd.command_type, cmd_types[i]);
            prop_assert_eq!(cmd.color, colors[i]);
            prop_assert_eq!(cmd.x, xs[i]);
            prop_assert_eq!(cmd.y, ys[i]);
            prop_assert_eq!(cmd.width, ws[i]);
            prop_assert_eq!(cmd.height, hs[i]);
            let content = doc.get_string(cmd.content_offset).unwrap();
            prop_assert_eq!(&content, &contents[i]);
        }
    }
}

// ─── Random Byte Fuzzing ─────────────────────────────────────────────────────

/// Property: random byte sequences must either parse successfully or return
/// a graceful error — they must NEVER panic.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn prop_random_bytes_never_panic(data in proptest::collection::vec(any::<u8>(), 0..500)) {
        // This test verifies the parser never panics on arbitrary input.
        let _result = NdaDocument::read(&data);
    }

    #[test]
    fn prop_random_bytes_with_valid_magic_never_panic(
        tail in proptest::collection::vec(any::<u8>(), 0..500),
    ) {
        let mut data = Vec::with_capacity(HEADER_SIZE + tail.len());
        data.extend_from_slice(&NDA_MAGIC.to_le_bytes());
        data.extend_from_slice(&tail);
        let _result = NdaDocument::read(&data);
    }
}

// ─── Merkle Integrity ────────────────────────────────────────────────────────

/// Property: any compiled document's Merkle root always verifies.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn prop_merkle_always_verifies(
        triples in proptest::collection::vec(
            (any::<String>(), any::<String>(), any::<String>()),
            1..20,
        ),
    ) {
        let mut compiler = NdaCompiler::new();
        for (s, p, o) in &triples {
            compiler.add_triple(s, p, o);
        }
        let data = compiler.compile();
        let doc = NdaDocument::read(&data).unwrap();
        prop_assert!(doc.verify_merkle().is_ok());
    }
}

// ─── Ed25519 Signature Fuzzing ───────────────────────────────────────────────

/// Property: sign → verify always succeeds for arbitrary documents.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_signature_always_verifies(
        triples in proptest::collection::vec(
            (any::<String>(), any::<String>(), any::<String>()),
            1..10,
        ),
        key_byte in any::<u8>(),
    ) {
        use ed25519_dalek::SigningKey;
        let signing_key = SigningKey::from_bytes(&[key_byte; 32]);

        let mut compiler = NdaCompiler::new();
        for (s, p, o) in &triples {
            compiler.add_triple(s, p, o);
        }
        let signed_data = compiler.compile_signed(&signing_key);
        prop_assert!(NdaDocument::verify_signature(&signed_data).is_ok());
    }

    #[test]
    fn prop_tampered_signature_always_fails(
        triples in proptest::collection::vec(
            (any::<String>(), any::<String>(), any::<String>()),
            1..5,
        ),
        key_byte in any::<u8>(),
        tamper_pos in 0usize..100,
    ) {
        use ed25519_dalek::SigningKey;
        let signing_key = SigningKey::from_bytes(&[key_byte; 32]);

        let mut compiler = NdaCompiler::new();
        for (s, p, o) in &triples {
            compiler.add_triple(s, p, o);
        }
        let mut signed_data = compiler.compile_signed(&signing_key);

        // Tamper with a byte in the signed content area (not the signature section)
        let content_len = signed_data.len() - SIGNATURE_SECTION_SIZE;
        if content_len > 0 {
            let pos = tamper_pos % content_len;
            signed_data[pos] ^= 0xFF;
            prop_assert!(NdaDocument::verify_signature(&signed_data).is_err());
        }
    }
}

// ─── String Pool Edge Cases ──────────────────────────────────────────────────

/// Property: strings with special characters survive round-trip.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_special_strings_round_trip(
        strings in proptest::collection::vec(
            "[\\x20-\\x7E]{0,200}",
            1..10,
        ),
    ) {
        let mut compiler = NdaCompiler::new();
        for s in &strings {
            compiler.add_triple(s, "PRED", "OBJ");
        }
        let data = compiler.compile();
        let doc = NdaDocument::read(&data).unwrap();

        for (i, triple) in doc.triples.iter().enumerate() {
            let s = doc.get_string(triple.subject_offset).unwrap();
            prop_assert_eq!(&s, &strings[i]);
        }
    }

    #[test]
    fn prop_unicode_strings_round_trip(
        strings in proptest::collection::vec(
            "\\PC{0,50}",
            1..5,
        ),
    ) {
        let mut compiler = NdaCompiler::new();
        for s in &strings {
            compiler.add_triple(s, "P", "O");
        }
        let data = compiler.compile();
        let doc = NdaDocument::read(&data).unwrap();

        for (i, triple) in doc.triples.iter().enumerate() {
            let s = doc.get_string(triple.subject_offset).unwrap();
            prop_assert_eq!(&s, &strings[i]);
        }
    }
}

// ─── Sandbox Edge Cases ──────────────────────────────────────────────────────

/// Property: sandbox always cleans up its temp directory.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_sandbox_always_cleans_up(
        file_name in "[a-zA-Z0-9_.-]{1,50}",
        file_content in proptest::collection::vec(any::<u8>(), 0..1000),
    ) {
        use velocity_mcp::sandbox::Sandbox;

        let work_dir;
        {
            let sandbox = match Sandbox::new() {
                Ok(s) => s,
                Err(_) => return Ok(()),
            };
            work_dir = sandbox.work_dir().to_path_buf();
            let _ = sandbox.write_file(&file_name, &file_content);
        }
        prop_assert!(!work_dir.exists(), "Sandbox dir should be cleaned up");
    }
}
