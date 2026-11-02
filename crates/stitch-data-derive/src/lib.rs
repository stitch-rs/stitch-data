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
    BitPack(u8),
}

struct FieldInfo<'a> {
    name: &'a syn::Ident,
    ty: &'a syn::Type,
    codec: CodecKind,
}

enum Segment<'a> {
    Single(FieldInfo<'a>),
    Bitstream(Vec<FieldInfo<'a>>),
}

/// Derives `CbpRecord` for a struct, providing zero-allocation serialization,
/// deserialization, sub-byte bit packing, and compile-time schema fingerprinting.
#[proc_macro_derive(StitchRecord, attributes(stitch, cbp))]
pub fn derive_stitch_record(input: TokenStream) -> TokenStream {
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

    let mut parsed_fields = Vec::new();
    let mut field_names = Vec::new();

    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        field_names.push(field_name);
        let field_type = &field.ty;

        // Parse attributes
        let mut codec = infer_default_codec(field_type);
        for attr in &field.attrs {
            if (attr.path().is_ident("stitch") || attr.path().is_ident("cbp"))
                && let Ok(meta_list) = attr.meta.require_list()
            {
                let mut parse_err = None;
                let _ = meta_list.parse_nested_meta(|meta| {
                    if meta.path.is_ident("delta") {
                        parse_err = Some(syn::Error::new_spanned(
                            &meta.path,
                            "#[cbp(delta)] is not supported on single records: delta compression is a sequence/columnar transform in cbp-transforms; use #[cbp(varint)] or #[cbp(raw)]",
                        ));
                        Ok(())
                    } else if meta.path.is_ident("gorilla") {
                        parse_err = Some(syn::Error::new_spanned(
                            &meta.path,
                            "#[cbp(gorilla)] is not supported on single records: Gorilla XOR compression is a sequence/columnar transform in cbp-transforms; use #[cbp(raw)]",
                        ));
                        Ok(())
                    } else if meta.path.is_ident("varint") {
                        codec = CodecKind::Varint;
                        Ok(())
                    } else if meta.path.is_ident("zigzag") {
                        codec = CodecKind::ZigZag;
                        Ok(())
                    } else if meta.path.is_ident("raw") {
                        codec = CodecKind::Raw;
                        Ok(())
                    } else if meta.path.is_ident("bits") {
                        let ty_str = quote!(#field_type).to_string();
                        let cleaned_ty = ty_str.trim();
                        if matches!(cleaned_ty, "i8" | "i16" | "i32" | "i64" | "isize") {
                            parse_err = Some(syn::Error::new_spanned(
                                field_type,
                                "#[cbp(bits = N)] is only supported on unsigned integer types (u8, u16, u32, u64, usize) and bool; for signed integers use #[cbp(zigzag)]",
                            ));
                        } else {
                            let value: Lit = meta.value()?.parse()?;
                            if let Lit::Int(int_lit) = value {
                                let width = int_lit.base10_parse::<u8>()?;
                                let max_bits = match cleaned_ty {
                                    "bool" => 1,
                                    "u8" => 8,
                                    "u16" => 16,
                                    "u32" => 32,
                                    "u64" | "usize" => 64,
                                    _ => 64,
                                };
                                if width == 0 || width > max_bits {
                                    parse_err = Some(syn::Error::new_spanned(
                                        &int_lit,
                                        format!("#[cbp(bits = {width})] invalid bit-width: must be between 1 and {max_bits} for type {cleaned_ty}"),
                                    ));
                                } else {
                                    codec = CodecKind::BitPack(width);
                                }
                            }
                        }
                        Ok(())
                    } else if meta.path.is_ident("codec") {
                        let expr: Expr = meta.value()?.parse()?;
                        let expr_str = quote!(#expr).to_string();
                        if expr_str.contains("Varint") {
                            codec = CodecKind::Varint;
                        } else if expr_str.contains("ZigZag") {
                            codec = CodecKind::ZigZag;
                        } else if expr_str.contains("Raw") {
                            codec = CodecKind::Raw;
                        }
                        Ok(())
                    } else {
                        Ok(())
                    }
                });

                if let Some(err) = parse_err {
                    return err.to_compile_error().into();
                }
            }
        }

        // Append field identifier and wire layout to schema string for deterministic hashing
        schema_string.push_str(&field_name.to_string());
        schema_string.push(':');
        schema_string.push_str(&type_to_schema_str(field_type));
        schema_string.push('@');
        schema_string.push_str(&codec_to_str(codec));
        schema_string.push(';');

        parsed_fields.push(FieldInfo {
            name: field_name,
            ty: field_type,
            codec,
        });
    }

    schema_string.push('}');
    let schema_hash_val = fnv1a_64(schema_string.as_bytes());

    // Group fields into segments (grouping contiguous bitpack fields together)
    let segments = group_into_segments(&parsed_fields);

    // Generate Direct profile statements (raw uncompressed POD serialization)
    let mut encode_direct_stmts = Vec::new();
    let mut decode_direct_stmts = Vec::new();
    for field in &parsed_fields {
        let (enc, dec) = generate_direct_field(field.name, field.ty);
        encode_direct_stmts.push(enc);
        decode_direct_stmts.push(dec);
    }

    // Generate Compact (Balanced/Realtime/MaxDensity) profile statements
    let mut encode_compact_stmts = Vec::new();
    let mut decode_compact_stmts = Vec::new();
    for seg in &segments {
        let (enc, dec) = generate_segment(seg);
        encode_compact_stmts.push(enc);
        decode_compact_stmts.push(dec);
    }

    let expanded = quote! {
        #[automatically_derived]
        impl ::stitch_data::record::StitchRecord for #struct_name {
            const SCHEMA_HASH: u64 = #schema_hash_val;

            #[inline]
            fn encode_record(&self, dst: &mut [u8]) -> ::core::result::Result<usize, ::stitch_data::error::CbpError> {
                self.encode_record_profile(::stitch_data::frame::OptimizationProfile::Balanced, dst)
            }

            #[inline]
            fn decode_record(src: &[u8]) -> ::core::result::Result<(Self, usize), ::stitch_data::error::CbpError> {
                Self::decode_record_profile(src, ::stitch_data::frame::OptimizationProfile::Balanced)
            }

            fn encode_record_profile(
                &self,
                profile: ::stitch_data::frame::OptimizationProfile,
                dst: &mut [u8],
            ) -> ::core::result::Result<usize, ::stitch_data::error::CbpError> {
                let mut offset = 0usize;
                match profile {
                    ::stitch_data::frame::OptimizationProfile::Direct => {
                        #( #encode_direct_stmts )*
                    }
                    _ => {
                        #( #encode_compact_stmts )*
                    }
                }
                Ok(offset)
            }

            fn decode_record_profile(
                src: &[u8],
                profile: ::stitch_data::frame::OptimizationProfile,
            ) -> ::core::result::Result<(Self, usize), ::stitch_data::error::CbpError> {
                let mut offset = 0usize;
                match profile {
                    ::stitch_data::frame::OptimizationProfile::Direct => {
                        #( #decode_direct_stmts )*
                        Ok((
                            Self {
                                #( #field_names ),*
                            },
                            offset,
                        ))
                    }
                    _ => {
                        #( #decode_compact_stmts )*
                        Ok((
                            Self {
                                #( #field_names ),*
                            },
                            offset,
                        ))
                    }
                }
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

fn group_into_segments<'a>(fields: &'a [FieldInfo<'a>]) -> Vec<Segment<'a>> {
    let mut segments = Vec::new();
    let mut current_bitpack = Vec::new();

    for field in fields {
        match field.codec {
            CodecKind::BitPack(_) => {
                current_bitpack.push(FieldInfo {
                    name: field.name,
                    ty: field.ty,
                    codec: field.codec,
                });
            }
            _ => {
                if !current_bitpack.is_empty() {
                    let run = std::mem::take(&mut current_bitpack);
                    segments.push(Segment::Bitstream(run));
                }
                segments.push(Segment::Single(FieldInfo {
                    name: field.name,
                    ty: field.ty,
                    codec: field.codec,
                }));
            }
        }
    }

    if !current_bitpack.is_empty() {
        segments.push(Segment::Bitstream(current_bitpack));
    }

    segments
}

fn generate_segment(seg: &Segment) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    match seg {
        Segment::Single(field) => generate_single_field(field.name, field.ty, field.codec),
        Segment::Bitstream(fields) => generate_bitstream_segment(fields),
    }
}

fn generate_bitstream_segment(
    fields: &[FieldInfo],
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let mut encode_writes = Vec::new();
    let mut decode_reads = Vec::new();

    for field in fields {
        let name = field.name;
        let ty = field.ty;
        let ty_str = quote!(#ty).to_string();
        let cleaned = ty_str.trim();
        let width = match field.codec {
            CodecKind::BitPack(w) => w,
            _ => unreachable!(),
        };

        let max_val: u64 = if width >= 64 {
            u64::MAX
        } else {
            (1u64 << width) - 1
        };

        let val_expr = if cleaned == "bool" {
            quote! { if self.#name { 1u64 } else { 0u64 } }
        } else {
            quote! { self.#name as u64 }
        };

        encode_writes.push(quote! {
            let val = #val_expr;
            if val > #max_val {
                return Err(::stitch_data::error::CbpError::InvalidBitWidth(#width));
            }
            bit_writer.write_bits(val, #width)?;
        });

        let decode_assignment = if cleaned == "bool" {
            quote! { let #name = raw != 0; }
        } else {
            quote! {
                let #name = <#ty>::try_from(raw).map_err(|_| ::stitch_data::error::CbpError::PayloadDecodeError)?;
            }
        };

        decode_reads.push(quote! {
            let raw = bit_reader.read_bits(#width)?;
            if raw > #max_val {
                return Err(::stitch_data::error::CbpError::InvalidBitWidth(#width));
            }
            #decode_assignment
        });
    }

    let enc = quote! {
        {
            let mut bit_writer = ::stitch_data::bitstream::BitWriter::new(&mut dst[offset..]);
            #( #encode_writes )*
            bit_writer.align();
            offset += bit_writer.bytes_written();
        }
    };

    let dec = quote! {
        let mut bit_reader = ::stitch_data::bitstream::BitReader::new(&src[offset..]);
        #( #decode_reads )*
        bit_reader.align();
        offset += bit_reader.bytes_read();
    };

    (enc, dec)
}

fn generate_single_field(
    field_name: &syn::Ident,
    field_type: &syn::Type,
    codec: CodecKind,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    match codec {
        CodecKind::Varint => {
            let enc = quote! {
                if dst.len() < offset + 9 {
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
                        required: offset + 9,
                        available: dst.len(),
                    });
                }
                let vlen = ::stitch_data::varint::encode_prefix_varint(self.#field_name as u64, &mut dst[offset..]);
                offset += vlen;
            };
            let dec = quote! {
                let (val, vlen) = ::stitch_data::varint::decode_prefix_varint(&src[offset..])?;
                offset += vlen;
                let #field_name = <#field_type>::try_from(val).map_err(|_| ::stitch_data::error::CbpError::MalformedVarint)?;
            };
            (enc, dec)
        }
        CodecKind::ZigZag => {
            let enc = quote! {
                if dst.len() < offset + 9 {
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
                        required: offset + 9,
                        available: dst.len(),
                    });
                }
                let zz = ::stitch_data::zigzag::encode_zigzag64(self.#field_name as i64);
                let vlen = ::stitch_data::varint::encode_prefix_varint(zz, &mut dst[offset..]);
                offset += vlen;
            };
            let dec = quote! {
                let (val, vlen) = ::stitch_data::varint::decode_prefix_varint(&src[offset..])?;
                offset += vlen;
                let zz = ::stitch_data::zigzag::decode_zigzag64(val);
                let #field_name = <#field_type>::try_from(zz).map_err(|_| ::stitch_data::error::CbpError::MalformedVarint)?;
            };
            (enc, dec)
        }
        CodecKind::Raw => generate_direct_field(field_name, field_type),
        CodecKind::BitPack(_) => unreachable!(),
    }
}

fn generate_direct_field(
    field_name: &syn::Ident,
    field_type: &syn::Type,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let ty_str = quote!(#field_type).to_string();
    let cleaned = ty_str.trim();

    match cleaned {
        "bool" => {
            let enc = quote! {
                if dst.len() < offset + 1 {
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
                        required: offset + 1,
                        available: dst.len(),
                    });
                }
                dst[offset] = if self.#field_name { 1 } else { 0 };
                offset += 1;
            };
            let dec = quote! {
                if src.len() < offset + 1 {
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
                        required: offset + 1,
                        available: src.len(),
                    });
                }
                let #field_name = src[offset] != 0;
                offset += 1;
            };
            (enc, dec)
        }
        "u8" => {
            let enc = quote! {
                if dst.len() < offset + 1 {
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
                        required: offset + 1,
                        available: dst.len(),
                    });
                }
                dst[offset] = self.#field_name;
                offset += 1;
            };
            let dec = quote! {
                if src.len() < offset + 1 {
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
                        required: offset + 1,
                        available: src.len(),
                    });
                }
                let #field_name = src[offset];
                offset += 1;
            };
            (enc, dec)
        }
        "i8" => {
            let enc = quote! {
                if dst.len() < offset + 1 {
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
                        required: offset + 1,
                        available: dst.len(),
                    });
                }
                dst[offset] = self.#field_name as u8;
                offset += 1;
            };
            let dec = quote! {
                if src.len() < offset + 1 {
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
                        required: offset + 1,
                        available: src.len(),
                    });
                }
                let #field_name = src[offset] as i8;
                offset += 1;
            };
            (enc, dec)
        }
        "u16" | "i16" => {
            let enc = quote! {
                if dst.len() < offset + 2 {
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
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
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
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
        "f32" | "u32" | "i32" => {
            let enc = quote! {
                if dst.len() < offset + 4 {
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
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
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
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
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
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
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
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
        "usize" => {
            let enc = quote! {
                if dst.len() < offset + 8 {
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
                        required: offset + 8,
                        available: dst.len(),
                    });
                }
                let le = (self.#field_name as u64).to_le_bytes();
                dst[offset..offset + 8].copy_from_slice(&le);
                offset += 8;
            };
            let dec = quote! {
                if src.len() < offset + 8 {
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
                        required: offset + 8,
                        available: src.len(),
                    });
                }
                let bytes: [u8; 8] = src[offset..offset + 8].try_into().unwrap();
                offset += 8;
                let raw = u64::from_le_bytes(bytes);
                let #field_name = usize::try_from(raw).map_err(|_| ::stitch_data::error::CbpError::PayloadDecodeError)?;
            };
            (enc, dec)
        }
        _ => {
            // Fallback: Varint for unknown integer types or raw bytes
            let enc = quote! {
                if dst.len() < offset + 9 {
                    return Err(::stitch_data::error::CbpError::UnexpectedEof {
                        required: offset + 9,
                        available: dst.len(),
                    });
                }
                let vlen = ::stitch_data::varint::encode_prefix_varint(self.#field_name as u64, &mut dst[offset..]);
                offset += vlen;
            };
            let dec = quote! {
                let (val, vlen) = ::stitch_data::varint::decode_prefix_varint(&src[offset..])?;
                offset += vlen;
                let #field_name = <#field_type>::try_from(val).map_err(|_| ::stitch_data::error::CbpError::MalformedVarint)?;
            };
            (enc, dec)
        }
    }
}

#[proc_macro_derive(CbpRecord, attributes(stitch, cbp))]
pub fn derive_cbp_record(input: TokenStream) -> TokenStream {
    derive_stitch_record(input)
}
