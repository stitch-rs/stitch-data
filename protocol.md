# Compact Binary Protocol (CBP) — Полная спецификация

Версия: 0.9
Дата: 2025-10-04

---

## 0. Коротко (mission)

CBP — контрактно-ориентированный двоичный протокол для сценариев с жёсткими требованиями к трафику и задержкам (игры, real-time telemetry, embedded). Цель: минимизировать трафик и CPU-cost, делая компромиссов минимум, через хорошо формализованные трансформации (переложенные в битовые флаги и параметры) и handshake-ориентированную сессию.

---

## 1. Концепты и уровни

CBP делит обработку данных на уровни:

* **Per-value** — трансформации отдельных примитивов (varint, zigzag, quantize, packed-bits).
* **Per-field / per-array** — операции над коллекциями (parallel arrays, RLE, Golomb-Rice).
* **Per-structure** — flattening, порядок полей.
* **Per-message** — сборка блоков, offset table, флаги сообщения.
* **Per-session (handshake)** — schema fingerprint, transform flags, params, dictionaries, codebooks.

Вся сложная логика конфигурируется через handshake → codegen делает fast-path сериализаторы/десериализаторы.

---

## 2. Handshake: что и зачем

Handshake выполняется один раз в начале сессии. Его задача — передать всё статическое, дорогое в передаче: схему, выбор pipeline’ов, таблицы параметров, словари и codebooks.

**Структура handshake (бинарно, примитивы varint/bytes):**

```
[Magic:4]                        -- опционально
[HandshakeLen:varint]
[SchemaFingerprint: u64]        -- xxHash64 или другой
[SchemaVersion: varint]
[NumFields:varint]
For i in 1..NumFields:
  [FieldId:varint]
  [FieldType:u8]                -- код примитива (INT, FLOAT, STRING, ARRAY, OBJECT...)
  [TransformFlags:u64]          -- битовая маска (bitsum)
  [ParamIndex:varint]           -- индекс в paramsBlock (0 = none)
[ParamsBlockLen:varint]
[ParamsBlock:bytes]             -- sequence (paramKey:uint8, paramValue:varint)*; может быть сжат
[NumDictionaries:varint]
... (dictionaries)
[NumCodebooks:varint]
... (codebooks)
[Capabilities:varint]            -- битмаска engine features (ZeroCopy, ANS, LZ4...)
[OptionalSecurityMeta...]
```

**Правила по bitsum / params:**

* `TransformFlags` (bitsum) — основной набор трансформаций для поля. Параметры, требующие аргументы, находятся в `ParamsBlock` и привязаны по `ParamIndex`.
* Handshake **обязательный** для Compact mode. Если клиент/сервер не могут согласовать schemaFingerprint — fallback в Compatible mode (тэги/len per-field).

**Компрессия handshake:** Handshake может быть сжат (например, deflate) — это допустимо, потому что это единоразовый налог и обе стороны знают supported codecs (Capabilities).

---

## 3. Message framing (per-message header)

Каждое сообщение начинается с мини-заголовка:

```
[MsgFlags: u8]        -- битовые флаги (has_offset_table,is_compressed,is_fragment,override_flags_present,...)
[MsgLen: varint]      -- длина payload (если используется транспорт без длины)
If MsgFlags.override_flags_present:
  [OverrideFlags: u64]      -- короткий override для TransformFlags (редко!)
  [OverrideParamIndex: varint] -- index in inline small param block (optional)
[Payload: bytes]       -- формат зависит от handshake (Compact/ZeroCopy/Compatible)
```

**Замечание:** override должен быть редкостью; handshake — место для постоянных определений. Override принимается только как временная эпизодическая мера.

---

## 4. TransformOp (bitsum) — битовые флаги

Все трансформации кодируются как биты в 64-битной маске `TransformFlags`. Ниже — рекомендуемый базовый набор (reserve bits for extension):

```
enum TransformOp : u64 {
  None                = 0,
  Varint              = 1 << 0,   // encode integer as varint
  ZigZag              = 1 << 1,   // apply ZigZag (signed->unsigned)
  Delta               = 1 << 2,   // encode delta to predictor
  Quantize            = 1 << 3,   // quantize float -> int
  PackedBits          = 1 << 4,   // pack small ints/booleans into bitstream
  RLE                 = 1 << 5,   // run-length encode
  GolombRice          = 1 << 6,   // use Golomb-Rice
  ANS_Entropy         = 1 << 7,   // use ANS with provided codebook
  ParallelArray       = 1 << 8,   // this field is part of parallel array group
  ZeroCopy            = 1 << 9,   // offset-table blocks for zero-copy
  Compressed_LZ4      = 1 << 10,  // LZ4 block compress (message/block level)
  PackedEnum          = 1 << 11,  // enums packed into minimal bits
  // reserve higher bits for predictors, modifiers, future codecs
}
```

**Параметры:** некоторые флаги требуют параметров (количество бит для Quantize, k для GolombRice, predictor type для Delta и т.д.). Параметры описываются в `ParamsBlock` и индексируются `ParamIndex`.

---

## 5. ParamsBlock — компактное хранение параметров

`ParamsBlock` — последовательность пар `(paramKey:uint8, paramValue:varint)`. Это позволяет связать набор параметров к одному или нескольким полям через `ParamIndex`. Примеры параметрKey:

* `0x01` = quantize_bits (u8)
* `0x02` = quantize_scale_index (u8) — индекс в предопределённой таблице scale factors
* `0x10` = predictor_type (u8) — 0=PrevValue,1=Linear,2=Velocity
* `0x20` = golomb_k (u8)
* `0x30` = packed_bits_per_element (u8)
* `0x40` = parallelGroupId (u16)
* `0x50` = zero_copy_threshold (varint)

`ParamsBlock` может быть поджат при handshake. Для экстренных per-message переопределений поддерживается маленький inline override param block (должен быть минимален).

---

## 6. Data section formats (modes)

### 6.1 Compact mode (recommended for same-version clients)

* No per-field tags. Fields are serialized in contract order, using per-field TransformFlags and parameters from handshake.
* For fixed-size fields/arrays: serialized inline without length.
* For variable-length arrays/strings: prepend `len` as compact varint.
* No offset table by default; ZeroCopy flag on a field indicates that blocks must be built with offset table if they exceed threshold.

### 6.2 Compatible mode (fallback for schema mismatch / public API)

* Tagged fields: each field encoded as `[fieldId:varint][TTLL marker/len][payload]`. Unknown fields are skipped.
* Safe for schema evolution.

### 6.3 ZeroCopy mode (for large payloads)

* Message payload contains offset table then data blocks:

  ```
  [OffsetCount:varint][Offset0:uint32][Offset1:uint32]...[DataBlocks...]
  ```
* Offsets point to start of blocks; blocks may be raw or compressed individually. Offset table enables decoder to reference blocks without copying entire message.

### 6.4 Entropy mode (ANS/Huffman)

* Handshake provides canonical codebook. Data section is encoded using ANS/Huffman. Usually preceded by local transforms (delta, quantize, RLE).
* Not compatible with raw offset-table across whole stream (can be mixed if blocks are encoded separately and offset points to compressed block).

---

## 7. Primitive encodings and order of transforms (mandatory)

**Order must be respected by all implementations.** Serializer applies transforms in this order; Deserializer reverses order.

1. **Quantize** (float → integer using quantize_bits and scale)
2. **Predictor/Delta** (value → residual relative to predictor)
3. **PackedBits** (accumulate small fields into bitstream)
4. **ZigZag** (signed→unsigned mapping)
5. **Varint** (LEB128-like)
6. **Group-level transforms** (RLE, Golomb-Rice)
7. **Entropy (ANS/Huffman)** (last lossless encoder before block-level compression)
8. **Block-level compression** (LZ4) — optional and applied at message/block level after above

**Пояснения:**

* Quantize before Delta: иначе residuals будут floating and worse compressed.
* Delta before Varint: small residuals compact well.
* RLE before Entropy: RLE improves entropy distribution for bitmaps.
* ANS/Huffman last: they expect symbol stream.

---

## 8. Arrays & flattening

### Fixed-size arrays

* If contract specifies count `N`, serialize elements inline: no length prefix. E.g. `Vec3` → `[X][Y][Z]`.

### Variable-length arrays

* If contract allows arbitrary length, encode as `[len:varint][elem1]...[elemN]`. Use `ParallelArray` flag + parallelGroupId for columnar encoding when beneficial.

### Parallel arrays

* For arrays of structs composed of primitives, use `ParallelArray` transform to emit N arrays (columns) instead of N rows. Efficient for cache and zero-copy.

---

## 9. Predictive transforms (Delta & Predictors)

* Predictor types enumerated in params (PrevValue, Linear, Velocity). Predictors must be deterministic and initialized consistently on both sides.
* For time-series: prefer delta-of-delta for timestamps and velocity-based predictor for positions.

---

## 10. Dictionary / Interning

* Handshake may include dictionaries (string list) or dictionary-id referencing server-side dictionary.
* Strings can be encoded as `interned index (varint)` or inline string if index==0.
* Dictionaries reduce repeated string overhead dramatically.

---

## 11. Micro-bytecode / Grammar-based compression (optional advanced)

* Optionally support a small bytecode for representing repeated fragments: opcodes like `REPEAT`, `COPY`, `FILL`, `EXPAND`.
* Bytecode definition shipped in handshake as capability + optional codebook. Use only if corpus shows repetitive long patterns.

---

## 12. Checksums, Integrity, Security

* Prefer transport-level TLS/DTLS for confidentiality/integrity.
* If in-protocol: do ephemeral key exchange in handshake and then AEAD (ChaCha20-Poly1305 / AES-GCM) on messages. Adds nonce+tag per message (overhead).
* Optional checksum (CRC32) flag for unreliable transports.

---

## 13. Handshake vs per-message overrides (bitsum policy)

* **Default**: `TransformFlags` and `ParamsBlock` are in handshake. This is the normal, most compact flow.
* **Overrides**: per-message override mask allowed (`OverrideFlags` + small param index) only when `MsgFlags` sets `override_flags_present`. Use it rarely; decoder must merge override with handshake flags deterministically (override bits replace handshake bits; param table merged accordingly).
* Never place large param blocks per-message.

---

## 14. Schema evolution & compatibility

* `SchemaFingerprint` identifies exact layout. If fingerprints identical → Compact mode allowed.
* If mismatch → negotiate Compatible mode: use tagged fields.
* **Field numbering**: never reuse field ids. Mark deprecated fields reserved.
* Add `sinceVersion` and `deprecated` meta in schema (for tooling).

---

## 15. Codegen & Testing rules (must)

* **Spec-driven codegen**: schema.json → auto-generate encoder/decoder for target languages (C#, TS, Rust, Pawn).
* **Deterministic behavior**: codegen ensures consistent ordering and transforms.
* **Tests**:

  * Roundtrip tests for each pipeline combination.
  * Golden bytes per schema for canonical check.
  * Performance & allocation benchmarks.
  * Fuzz tests for corrupted inputs.

---

## 16. Examples (byte-level sketches)

### Example Handshake fragment (conceptual)

```
Magic: "CBP\0"
SchemaFingerprint: 0x1f8a9b...
NumFields: 3
Field1: id=1, type=INT32, flags=Varint
Field2: id=2, type=FLOAT, flags=Quantize|Delta|ZigZag|Varint, paramIndex=1
Field3: id=3, type=string, flags=Varint (len) | Interned
ParamsBlock:
  [0x01=16]  // quantize_bits=16
  [0x02=3]   // quantize_scale_index=3
Dictionaries: itemNames...
Capabilities: ZeroCopy|ANS
```

### Example Compact message (player update)

Fields order (compact): playerId, posX,posY,posZ,yaw,flags,health

* playerId: varint
* posX: quantize->delta->zz->varint
* ... etc

Typical size: 8–20 bytes.

---

## 17. Pipelines (preset named pipelines)

* `MinimalCompact` — Varint + ZigZag + PackedBits (default)
* `DeltaCompact` — Quantize + Delta + ZigZag + Varint
* `ParallelColumnar` — Parallel arrays + per-column transforms
* `EntropySession` — local transforms + ANS with session codebook
* `ZeroCopyHybrid` — block offsets + optional block-level compression

---

## 18. Benchmarks & measurement plan

* Microbench: serialize/deserialize 1M small messages (1–32 bytes) — measure latency & allocations.
* Macrobench: simulate realistic server load and measure bandwidth savings vs ProtoBuf/MessagePack/FlatBuffers.
* Corpus-driven: build codebooks/dictionaries from real payloads and evaluate Entropy gains.

---

## 19. Roadmap и порядок реализации

1. Finalize `protocol.md` (this doc).
2. Implement schema JSON format + handshake generator.
3. Implement `MinimalCompact` pipeline + codegen (C#).
4. Unit tests (golden bytes) + microbench.
5. Add `DeltaCompact` and `ParallelArray`.
6. Implement ZeroCopy offset table and param thresholds.
7. Implement dictionaries and optional ANS/Huffman via handshake.
8. Optional: micro-bytecode engine if corpus justifies.

---

## 20. Appendix — reference: ZigZag + Varint

* ZigZag encode: `zz = (n << 1) ^ (n >> (bits-1))`
* Varint: 7 bits per byte with MSB continuation.

---

## 21. FAQ (коротко)

* **Q:** Где хранить bitsum?
  **A:** В handshake (основной), overrides per-message only if necessary.
* **Q:** ZeroCopy + ANS возможны?
  **A:** Да, но только если offset points to compressed blocks and decoder can lazily decompress. Cross-block ANS breaks random access.
* **Q:** Что делать если schema mismatch?
  **A:** Fall back to Compatible (tagged) mode.

---

Конец `protocol.md`.
Этот документ — canonical spec для v0.9. В него можно добавлять новые опции и расширять enum флагов (зарезервированы биты).
