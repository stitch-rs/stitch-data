//! Procedural derive macro for Compact Binary Protocol (`#[derive(CbpRecord)]`).

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Expr, Fields, Lit, Type, parse_macro_input};

/// 64-bit FNV-1a compile-time hashing helper.
const fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    let mut i = 0;
    while i < data.len() {
        hash ^= data[i] as u64;
        hash = hash.wrapping_mul(0x100000001b3);
        i += 1;
    }
    hash
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodecKind {
    Raw,
    Varint,
    ZigZag,
    Delta,
    Gorilla,
    BitPack(u8),
}

/// Derives `CbpRecord` for a struct, providing zero-allocation serialization,
/// deserialization, and compile-time schema fingerprinting.
#[proc_macro_derive(CbpRecord, attributes(cbp))]
pub fn derive_cbp_record(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;

    let fields = match &input.data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(fields_named) => &fields_named.named,
            _ => {
                return syn::Error::new_spanned(
                    struct_name,
                    "CbpRecord derive only supports structs with named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        _ => {
            return syn::Error::new_spanned(struct_name, "CbpRecord derive only supports structs")
                .to_compile_error()
                .into();
        }
    };

    let mut schema_string = std::string::String::new();
    schema_string.push_str(&struct_name.to_string());
    schema_string.push('{');

    let mut encode_stmts = std::vec::Vec::new();
    let mut decode_stmts = std::vec::Vec::new();
    let mut field_names = std::vec::Vec::new();

    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        field_names.push(field_name);
        let field_type = &field.ty;

        // Parse attributes
        let mut codec = infer_default_codec(field_type);
        for attr in &field.attrs {
            if attr.path().is_ident("cbp")
                && let Ok(meta_list) = attr.meta.require_list()
            {
                let _ = meta_list.parse_nested_meta(|meta| {
                    if meta.path.is_ident("delta") {
                        codec = CodecKind::Delta;
                        Ok(())
                    } else if meta.path.is_ident("varint") {
                        codec = CodecKind::Varint;
                        Ok(())
                    } else if meta.path.is_ident("zigzag") {
                        codec = CodecKind::ZigZag;
                        Ok(())
                    } else if meta.path.is_ident("gorilla") {
                        codec = CodecKind::Gorilla;
                        Ok(())
                    } else if meta.path.is_ident("raw") {
                        codec = CodecKind::Raw;
                        Ok(())
                    } else if meta.path.is_ident("bits") {
                        let value: Lit = meta.value()?.parse()?;
                        if let Lit::Int(int_lit) = value {
                            let width = int_lit.base10_parse::<u8>()?;
                            codec = CodecKind::BitPack(width);
                        }
                        Ok(())
                    } else if meta.path.is_ident("codec") {
                        // Support #[cbp(codec = FieldCodec::...)]
                        let expr: Expr = meta.value()?.parse()?;
                        let expr_str = quote!(#expr).to_string();
                        if expr_str.contains("Delta") {
                            codec = CodecKind::Delta;
                        } else if expr_str.contains("Varint") {
                            codec = CodecKind::Varint;
                        } else if expr_str.contains("ZigZag") {
                            codec = CodecKind::ZigZag;
                        } else if expr_str.contains("Gorilla") {
                            codec = CodecKind::Gorilla;
                        } else if expr_str.contains("Raw") {
                            codec = CodecKind::Raw;
                        }
                        Ok(())
                    } else {
                        Ok(())
                    }
                });
            }
        }

        // Append to schema string for deterministic hashing
        schema_string.push_str(&field_name.to_string());
        schema_string.push(':');
        schema_string.push_str(&type_to_schema_str(field_type));
        schema_string.push('@');
        schema_string.push_str(&codec_to_str(codec));
        schema_string.push(';');

        // Generate encode and decode logic per field
        let (enc, dec) = generate_field_codecs(field_name, field_type, codec);
        encode_stmts.push(enc);
        decode_stmts.push(dec);
    }

    schema_string.push('}');
    let schema_hash_val = fnv1a_64(schema_string.as_bytes());

    let expanded = quote! {
        #[automatically_derived]
        impl ::cbp::record::CbpRecord for #struct_name {
            const SCHEMA_HASH: u64 = #schema_hash_val;

            fn encode_record(&self, dst: &mut [u8]) -> ::core::result::Result<usize, ::cbp::error::CbpError> {
                let mut offset = 0usize;
                #( #encode_stmts )*
                Ok(offset)
            }

            fn decode_record(src: &[u8]) -> ::core::result::Result<(Self, usize), ::cbp::error::CbpError> {
                let mut offset = 0usize;
                #( #decode_stmts )*
                Ok((
                    Self {
                        #( #field_names ),*
                    },
                    offset,
                ))
            }
        }
    };

    expanded.into()
}

fn type_to_schema_str(ty: &Type) -> std::string::String {
    quote!(#ty).to_string().replace(' ', "")
}

fn codec_to_str(c: CodecKind) -> std::string::String {
    match c {
        CodecKind::Raw => std::string::String::from("raw"),
        CodecKind::Varint => std::string::String::from("varint"),
        CodecKind::ZigZag => std::string::String::from("zigzag"),
        CodecKind::Delta => std::string::String::from("delta"),
        CodecKind::Gorilla => std::string::String::from("gorilla"),
        CodecKind::BitPack(w) => format!("bits{w}"),
    }
}

fn infer_default_codec(ty: &Type) -> CodecKind {
    let ty_str = quote!(#ty).to_string();
    let cleaned = ty_str.trim();

    match cleaned {
        "u16" | "u32" | "u64" | "usize" => CodecKind::Varint,
        "i16" | "i32" | "i64" | "isize" => CodecKind::ZigZag,
        "f32" | "f64" => CodecKind::Raw,
        "bool" | "u8" | "i8" => CodecKind::Raw,
        _ => CodecKind::Raw,
    }
}

fn generate_field_codecs(
    field_name: &syn::Ident,
    field_type: &syn::Type,
    codec: CodecKind,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let ty_str = quote!(#field_type).to_string();
    let cleaned = ty_str.trim();

    match codec {
        CodecKind::Varint => {
            let enc = quote! {
                if dst.len() < offset + 9 {
                    return Err(::cbp::error::CbpError::UnexpectedEof {
                        required: offset + 9,
                        available: dst.len(),
                    });
                }
                let vlen = ::cbp::varint::encode_prefix_varint(self.#field_name as u64, &mut dst[offset..]);
                offset += vlen;
            };
            let dec = quote! {
                let (val, vlen) = ::cbp::varint::decode_prefix_varint(&src[offset..])?;
                offset += vlen;
                let #field_name = val as #field_type;
            };
            (enc, dec)
        }
        CodecKind::ZigZag => {
            let enc = quote! {
                if dst.len() < offset + 9 {
                    return Err(::cbp::error::CbpError::UnexpectedEof {
                        required: offset + 9,
                        available: dst.len(),
                    });
                }
                let zz = ::cbp::zigzag::encode_zigzag64(self.#field_name as i64);
                let vlen = ::cbp::varint::encode_prefix_varint(zz, &mut dst[offset..]);
                offset += vlen;
            };
            let dec = quote! {
                let (val, vlen) = ::cbp::varint::decode_prefix_varint(&src[offset..])?;
                offset += vlen;
                let #field_name = ::cbp::zigzag::decode_zigzag64(val) as #field_type;
            };
            (enc, dec)
        }
        CodecKind::Delta => {
            // For single-record, delta from 0 is PrefixVarint
            let enc = quote! {
                if dst.len() < offset + 9 {
                    return Err(::cbp::error::CbpError::UnexpectedEof {
                        required: offset + 9,
                        available: dst.len(),
                    });
                }
                let vlen = ::cbp::varint::encode_prefix_varint(self.#field_name as u64, &mut dst[offset..]);
                offset += vlen;
            };
            let dec = quote! {
                let (val, vlen) = ::cbp::varint::decode_prefix_varint(&src[offset..])?;
                offset += vlen;
                let #field_name = val as #field_type;
            };
            (enc, dec)
        }
        CodecKind::Gorilla | CodecKind::Raw => {
            match cleaned {
                "bool" => {
                    let enc = quote! {
                        if dst.len() < offset + 1 {
                            return Err(::cbp::error::CbpError::UnexpectedEof {
                                required: offset + 1,
                                available: dst.len(),
                            });
                        }
                        dst[offset] = if self.#field_name { 1 } else { 0 };
                        offset += 1;
                    };
                    let dec = quote! {
                        if src.len() < offset + 1 {
                            return Err(::cbp::error::CbpError::UnexpectedEof {
                                required: offset + 1,
                                available: src.len(),
                            });
                        }
                        let #field_name = src[offset] != 0;
                        offset += 1;
                    };
                    (enc, dec)
                }
                "u8" | "i8" => {
                    let enc = quote! {
                        if dst.len() < offset + 1 {
                            return Err(::cbp::error::CbpError::UnexpectedEof {
                                required: offset + 1,
                                available: dst.len(),
                            });
                        }
                        dst[offset] = self.#field_name as u8;
                        offset += 1;
                    };
                    let dec = quote! {
                        if src.len() < offset + 1 {
                            return Err(::cbp::error::CbpError::UnexpectedEof {
                                required: offset + 1,
                                available: src.len(),
                            });
                        }
                        let #field_name = src[offset] as #field_type;
                        offset += 1;
                    };
                    (enc, dec)
                }
                "f32" | "u32" | "i32" => {
                    let enc = quote! {
                        if dst.len() < offset + 4 {
                            return Err(::cbp::error::CbpError::UnexpectedEof {
                                required: offset + 4,
                                available: dst.len(),
                            });
                        }
                        let le = self.#field_name.to_le_bytes();
                        dst[offset..offset + 4].copy_from_slice(&le);
                        offset += 4;
                    };
                    let dec = quote! {
                        if src.len() < offset + 4 {
                            return Err(::cbp::error::CbpError::UnexpectedEof {
                                required: offset + 4,
                                available: src.len(),
                            });
                        }
                        let bytes: [u8; 4] = src[offset..offset + 4].try_into().unwrap();
                        offset += 4;
                        let #field_name = <#field_type>::from_le_bytes(bytes);
                    };
                    (enc, dec)
                }
                "f64" | "u64" | "i64" => {
                    let enc = quote! {
                        if dst.len() < offset + 8 {
                            return Err(::cbp::error::CbpError::UnexpectedEof {
                                required: offset + 8,
                                available: dst.len(),
                            });
                        }
                        let le = self.#field_name.to_le_bytes();
                        dst[offset..offset + 8].copy_from_slice(&le);
                        offset += 8;
                    };
                    let dec = quote! {
                        if src.len() < offset + 8 {
                            return Err(::cbp::error::CbpError::UnexpectedEof {
                                required: offset + 8,
                                available: src.len(),
                            });
                        }
                        let bytes: [u8; 8] = src[offset..offset + 8].try_into().unwrap();
                        offset += 8;
                        let #field_name = <#field_type>::from_le_bytes(bytes);
                    };
                    (enc, dec)
                }
                "u16" | "i16" => {
                    let enc = quote! {
                        if dst.len() < offset + 2 {
                            return Err(::cbp::error::CbpError::UnexpectedEof {
                                required: offset + 2,
                                available: dst.len(),
                            });
                        }
                        let le = self.#field_name.to_le_bytes();
                        dst[offset..offset + 2].copy_from_slice(&le);
                        offset += 2;
                    };
                    let dec = quote! {
                        if src.len() < offset + 2 {
                            return Err(::cbp::error::CbpError::UnexpectedEof {
                                required: offset + 2,
                                available: src.len(),
                            });
                        }
                        let bytes: [u8; 2] = src[offset..offset + 2].try_into().unwrap();
                        offset += 2;
                        let #field_name = <#field_type>::from_le_bytes(bytes);
                    };
                    (enc, dec)
                }
                _ => {
                    // Fallback: Varint for integers or raw bytes
                    let enc = quote! {
                        if dst.len() < offset + 9 {
                            return Err(::cbp::error::CbpError::UnexpectedEof {
                                required: offset + 9,
                                available: dst.len(),
                            });
                        }
                        let vlen = ::cbp::varint::encode_prefix_varint(self.#field_name as u64, &mut dst[offset..]);
                        offset += vlen;
                    };
                    let dec = quote! {
                        let (val, vlen) = ::cbp::varint::decode_prefix_varint(&src[offset..])?;
                        offset += vlen;
                        let #field_name = val as #field_type;
                    };
                    (enc, dec)
                }
            }
        }
        CodecKind::BitPack(_w) => {
            // For single-record, bounded integers encode into PrefixVarint or raw u8
            match cleaned {
                "u8" | "i8" => {
                    let enc = quote! {
                        if dst.len() < offset + 1 {
                            return Err(::cbp::error::CbpError::UnexpectedEof {
                                required: offset + 1,
                                available: dst.len(),
                            });
                        }
                        dst[offset] = self.#field_name as u8;
                        offset += 1;
                    };
                    let dec = quote! {
                        if src.len() < offset + 1 {
                            return Err(::cbp::error::CbpError::UnexpectedEof {
                                required: offset + 1,
                                available: src.len(),
                            });
                        }
                        let #field_name = src[offset] as #field_type;
                        offset += 1;
                    };
                    (enc, dec)
                }
                _ => {
                    let enc = quote! {
                        if dst.len() < offset + 9 {
                            return Err(::cbp::error::CbpError::UnexpectedEof {
                                required: offset + 9,
                                available: dst.len(),
                            });
                        }
                        let vlen = ::cbp::varint::encode_prefix_varint(self.#field_name as u64, &mut dst[offset..]);
                        offset += vlen;
                    };
                    let dec = quote! {
                        let (val, vlen) = ::cbp::varint::decode_prefix_varint(&src[offset..])?;
                        offset += vlen;
                        let #field_name = val as #field_type;
                    };
                    (enc, dec)
                }
            }
        }
    }
}
