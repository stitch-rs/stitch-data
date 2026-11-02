//! Schema evolution rules and schema compatibility verification.
//!
//! Provides deterministic verification of forward/backward field compatibility:
//! - Field addition: safe if new fields are appended or have default value fallbacks.
//! - Field type/codec compatibility: verifies bit-width and type invariant preservation.
//! - Schema fingerprint checks against wire frame headers.

use crate::error::StitchError;

/// Schema field descriptor metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDescriptor {
    /// Unique 1-based numeric field ID.
    pub id: u32,
    /// Canonical field name.
    pub name: &'static str,
    /// Type descriptor string (e.g. "u32", "f32", "i64").
    pub type_name: &'static str,
    /// Bit width or 0 for variable length.
    pub bit_width: u8,
}

/// Static schema definition for a `Record`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaDefinition {
    /// Fully qualified struct name.
    pub struct_name: &'static str,
    /// 64-bit schema hash.
    pub schema_hash: u64,
    /// Ordered slice of field descriptors.
    pub fields: &'static [FieldDescriptor],
}

/// Result of schema evolution compatibility evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityResult {
    /// Schemas are bit-exact identical.
    Identical,
    /// Target schema is a fully backwards-compatible superset of source schema.
    CompatibleSubset,
    /// Schemas have conflicting or removed fields, causing wire incompatibility.
    Incompatible(&'static str),
}

/// Verifier assessing schema evolution rules across protocol versions.
pub struct SchemaCompatibilityVerifier;

impl SchemaCompatibilityVerifier {
    /// Compares a reader schema against a writer schema.
    pub fn verify(reader: &SchemaDefinition, writer: &SchemaDefinition) -> CompatibilityResult {
        if reader.schema_hash == writer.schema_hash {
            return CompatibilityResult::Identical;
        }

        // Reader must recognize all existing writer fields with matching types
        for writer_field in writer.fields {
            let reader_field = reader.fields.iter().find(|f| f.id == writer_field.id);
            match reader_field {
                None => {
                    return CompatibilityResult::Incompatible(
                        "Writer contains field ID unknown to reader",
                    );
                }
                Some(rf) => {
                    if rf.type_name != writer_field.type_name {
                        return CompatibilityResult::Incompatible(
                            "Field type mismatch between reader and writer",
                        );
                    }
                    if rf.bit_width != writer_field.bit_width {
                        return CompatibilityResult::Incompatible(
                            "Field bit-width mismatch between reader and writer",
                        );
                    }
                }
            }
        }

        CompatibilityResult::CompatibleSubset
    }

    /// Validates that an incoming frame header's schema hash matches expected hash or is compatible.
    #[inline]
    pub fn validate_frame_schema(
        expected_hash: u64,
        actual_hash: Option<u64>,
    ) -> Result<(), StitchError> {
        match actual_hash {
            Some(actual) if actual != expected_hash => Err(StitchError::SchemaMismatch {
                expected: expected_hash,
                actual,
            }),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static FIELD_V1: [FieldDescriptor; 2] = [
        FieldDescriptor {
            id: 1,
            name: "id",
            type_name: "u32",
            bit_width: 32,
        },
        FieldDescriptor {
            id: 2,
            name: "price",
            type_name: "f64",
            bit_width: 64,
        },
    ];

    static FIELD_V2: [FieldDescriptor; 3] = [
        FieldDescriptor {
            id: 1,
            name: "id",
            type_name: "u32",
            bit_width: 32,
        },
        FieldDescriptor {
            id: 2,
            name: "price",
            type_name: "f64",
            bit_width: 64,
        },
        FieldDescriptor {
            id: 3,
            name: "volume",
            type_name: "u32",
            bit_width: 32,
        },
    ];

    static SCHEMA_V1: SchemaDefinition = SchemaDefinition {
        struct_name: "OrderTick",
        schema_hash: 0x1111_2222_3333_4444,
        fields: &FIELD_V1,
    };

    static SCHEMA_V2: SchemaDefinition = SchemaDefinition {
        struct_name: "OrderTick",
        schema_hash: 0x5555_6666_7777_8888,
        fields: &FIELD_V2,
    };

    #[test]
    fn test_schema_evolution_compatibility() {
        // V1 reader reading V1 writer -> Identical
        assert_eq!(
            SchemaCompatibilityVerifier::verify(&SCHEMA_V1, &SCHEMA_V1),
            CompatibilityResult::Identical
        );

        // V2 reader reading V1 writer -> CompatibleSubset (new fields added)
        assert_eq!(
            SchemaCompatibilityVerifier::verify(&SCHEMA_V2, &SCHEMA_V1),
            CompatibilityResult::CompatibleSubset
        );

        // V1 reader reading V2 writer -> Incompatible (writer has field ID 3 unknown to V1)
        assert!(matches!(
            SchemaCompatibilityVerifier::verify(&SCHEMA_V1, &SCHEMA_V2),
            CompatibilityResult::Incompatible(_)
        ));
    }
}
