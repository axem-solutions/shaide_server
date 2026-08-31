# Third-Party Software Licenses

**shaide Server Version:** 0.5.0

---

## Overview

This document lists all third-party dependencies used in the shaide server,
including their licenses and source code repositories.

---

## Git Dependencies

These dependencies are sourced directly from Git repositories:

| Package | Version/Rev  | License           | Repository                       |
| ------- | ------------ | ----------------- | -------------------------------- |
| sqlx    | rev: 8ca573c | MIT OR Apache-2.0 | https://github.com/wsxiaoys/sqlx |

---

## AI/ML & HTTP Clients

| Package                    | Version | License    | Repository                                                |
| -------------------------- | ------- | ---------- | --------------------------------------------------------- |
| async-openai               | 0.30.1  | MIT        | https://github.com/64bit/async-openai                     |
| async-openai-macros        | 0.1.0   | MIT        | https://github.com/64bit/async-openai                     |
| google-cloud-aiplatform-v1 | 1.1.0   | Apache-2.0 | https://github.com/googleapis/google-cloud-rust/tree/main |

## Web Framework & HTTP

| Package                | Version | License                  | Repository                                  |
| ---------------------- | ------- | ------------------------ | ------------------------------------------- |
| aws-smithy-http        | 0.62.6  | Apache-2.0               | https://github.com/smithy-lang/smithy-rs    |
| aws-smithy-http-client | 1.1.5   | Apache-2.0               | https://github.com/smithy-lang/smithy-rs    |
| axum                   | 0.7.9   | MIT                      | https://github.com/tokio-rs/axum            |
| axum-core              | 0.4.5   | MIT                      | https://github.com/tokio-rs/axum            |
| axum-extra             | 0.9.6   | MIT                      | https://github.com/tokio-rs/axum            |
| axum-macros            | 0.4.2   | MIT                      | https://github.com/tokio-rs/axum            |
| axum-prometheus        | 0.6.1   | MIT                      | https://github.com/Ptrskay3/axum-prometheus |
| http                   | 0.2.12  | MIT OR Apache-2.0        | https://github.com/hyperium/http            |
| http                   | 1.3.1   | MIT OR Apache-2.0        | https://github.com/hyperium/http            |
| http-body              | 0.4.6   | MIT                      | https://github.com/hyperium/http-body       |
| http-body              | 1.0.1   | MIT                      | https://github.com/hyperium/http-body       |
| http-body-util         | 0.1.3   | MIT                      | https://github.com/hyperium/http-body       |
| httparse               | 1.10.1  | MIT OR Apache-2.0        | https://github.com/seanmonstar/httparse     |
| httpdate               | 1.0.3   | MIT OR Apache-2.0        | https://github.com/pyfisch/httpdate         |
| hyper                  | 0.14.32 | MIT                      | https://github.com/hyperium/hyper           |
| hyper                  | 1.7.0   | MIT                      | https://github.com/hyperium/hyper           |
| hyper-rustls           | 0.24.2  | Apache-2.0 OR ISC OR MIT | https://github.com/rustls/hyper-rustls      |
| hyper-rustls           | 0.27.7  | Apache-2.0 OR ISC OR MIT | https://github.com/rustls/hyper-rustls      |
| hyper-timeout          | 0.5.2   | MIT OR Apache-2.0        | https://github.com/hjr3/hyper-timeout       |
| hyper-tls              | 0.6.0   | MIT/Apache-2.0           | https://github.com/hyperium/hyper-tls       |
| hyper-util             | 0.1.17  | MIT                      | https://github.com/hyperium/hyper-util      |

## AWS SDK & Cloud

| Package                  | Version | License                                 | Repository                               |
| ------------------------ | ------- | --------------------------------------- | ---------------------------------------- |
| aws-credential-types     | 1.2.11  | Apache-2.0                              | https://github.com/smithy-lang/smithy-rs |
| aws-lc-rs                | 1.15.2  | ISC AND (Apache-2.0 OR ISC)             | https://github.com/aws/aws-lc-rs         |
| aws-lc-sys               | 0.35.0  | ISC AND (Apache-2.0 OR ISC) AND OpenSSL | https://github.com/aws/aws-lc-rs         |
| aws-runtime              | 1.5.17  | Apache-2.0                              | https://github.com/smithy-lang/smithy-rs |
| aws-sdk-s3               | 1.119.0 | Apache-2.0                              | https://github.com/awslabs/aws-sdk-rust  |
| aws-sigv4                | 1.3.7   | Apache-2.0                              | https://github.com/smithy-lang/smithy-rs |
| aws-smithy-async         | 1.2.7   | Apache-2.0                              | https://github.com/smithy-lang/smithy-rs |
| aws-smithy-checksums     | 0.63.12 | Apache-2.0                              | https://github.com/smithy-lang/smithy-rs |
| aws-smithy-eventstream   | 0.60.14 | Apache-2.0                              | https://github.com/smithy-lang/smithy-rs |
| aws-smithy-json          | 0.61.9  | Apache-2.0                              | https://github.com/smithy-lang/smithy-rs |
| aws-smithy-observability | 0.1.5   | Apache-2.0                              | https://github.com/awslabs/smithy-rs     |
| aws-smithy-runtime       | 1.9.5   | Apache-2.0                              | https://github.com/smithy-lang/smithy-rs |
| aws-smithy-runtime-api   | 1.9.3   | Apache-2.0                              | https://github.com/smithy-lang/smithy-rs |
| aws-smithy-types         | 1.3.5   | Apache-2.0                              | https://github.com/smithy-lang/smithy-rs |
| aws-smithy-xml           | 0.60.13 | Apache-2.0                              | https://github.com/smithy-lang/smithy-rs |
| aws-types                | 1.3.11  | Apache-2.0                              | https://github.com/smithy-lang/smithy-rs |

## Google Cloud

| Package                   | Version | License    | Repository                                                |
| ------------------------- | ------- | ---------- | --------------------------------------------------------- |
| google-cloud-api          | 1.1.0   | Apache-2.0 | https://github.com/googleapis/google-cloud-rust/tree/main |
| google-cloud-auth         | 1.0.1   | Apache-2.0 | https://github.com/googleapis/google-cloud-rust/tree/main |
| google-cloud-gax          | 1.1.0   | Apache-2.0 | https://github.com/googleapis/google-cloud-rust/tree/main |
| google-cloud-gax-internal | 0.7.1   | Apache-2.0 | https://github.com/googleapis/google-cloud-rust/tree/main |
| google-cloud-iam-v1       | 1.1.0   | Apache-2.0 | https://github.com/googleapis/google-cloud-rust/tree/main |
| google-cloud-location     | 1.1.0   | Apache-2.0 | https://github.com/googleapis/google-cloud-rust/tree/main |
| google-cloud-longrunning  | 1.1.0   | Apache-2.0 | https://github.com/googleapis/google-cloud-rust/tree/main |
| google-cloud-lro          | 1.0.0   | Apache-2.0 | https://github.com/googleapis/google-cloud-rust/tree/main |
| google-cloud-rpc          | 1.1.0   | Apache-2.0 | https://github.com/googleapis/google-cloud-rust/tree/main |
| google-cloud-type         | 1.1.0   | Apache-2.0 | https://github.com/googleapis/google-cloud-rust/tree/main |
| google-cloud-wkt          | 1.1.0   | Apache-2.0 | https://github.com/googleapis/google-cloud-rust/tree/main |

## Serialization & Data

| Package | Version | License | Repository                       |
| ------- | ------- | ------- | -------------------------------- |
| bincode | 1.3.3   | MIT     | https://github.com/servo/bincode |

## Cryptography & Security

| Package              | Version | License           | Repository                                                       |
| -------------------- | ------- | ----------------- | ---------------------------------------------------------------- |
| argon2               | 0.5.3   | MIT OR Apache-2.0 | https://github.com/RustCrypto/password-hashes/tree/master/argon2 |
| blake2               | 0.10.6  | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes                             |
| crypto-bigint        | 0.4.9   | Apache-2.0 OR MIT | https://github.com/RustCrypto/crypto-bigint                      |
| crypto-bigint        | 0.5.5   | Apache-2.0 OR MIT | https://github.com/RustCrypto/crypto-bigint                      |
| crypto-common        | 0.1.6   | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits                             |
| digest               | 0.10.7  | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits                             |
| foreign-types-shared | 0.1.1   | MIT/Apache-2.0    | https://github.com/sfackler/foreign-types                        |
| hmac                 | 0.12.1  | MIT OR Apache-2.0 | https://github.com/RustCrypto/MACs                               |

## Async Runtime & Utilities

| Package           | Version | License           | Repository                                       |
| ----------------- | ------- | ----------------- | ------------------------------------------------ |
| async-stream      | 0.3.6   | MIT               | https://github.com/tokio-rs/async-stream         |
| async-stream-impl | 0.3.6   | MIT               | https://github.com/tokio-rs/async-stream         |
| async-trait       | 0.1.89  | MIT OR Apache-2.0 | https://github.com/dtolnay/async-trait           |
| futures           | 0.3.31  | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs          |
| futures-channel   | 0.3.31  | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs          |
| futures-core      | 0.3.31  | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs          |
| futures-executor  | 0.3.31  | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs          |
| futures-intrusive | 0.5.0   | MIT OR Apache-2.0 | https://github.com/Matthias247/futures-intrusive |
| futures-io        | 0.3.31  | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs          |
| futures-macro     | 0.3.31  | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs          |
| futures-sink      | 0.3.31  | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs          |
| futures-task      | 0.3.31  | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs          |
| futures-timer     | 3.0.3   | MIT/Apache-2.0    | https://github.com/async-rs/futures-timer        |
| futures-util      | 0.3.31  | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs          |

## Text Processing & Search

| Package        | Version | License           | Repository                                 |
| -------------- | ------- | ----------------- | ------------------------------------------ |
| aho-corasick   | 1.1.3   | Unlicense OR MIT  | https://github.com/BurntSushi/aho-corasick |
| encode_unicode | 1.0.0   | Apache-2.0 OR MIT | https://github.com/tormol/encode_unicode   |

## Telemetry & Observability

| Package     | Version | License           | Repository                                  |
| ----------- | ------- | ----------------- | ------------------------------------------- |
| crc-catalog | 2.4.0   | MIT OR Apache-2.0 | https://github.com/akhilles/crc-catalog.git |

## Command Line Interface

| Package       | Version | License           | Repository                                  |
| ------------- | ------- | ----------------- | ------------------------------------------- |
| clap          | 4.5.48  | MIT OR Apache-2.0 | https://github.com/clap-rs/clap             |
| clap_builder  | 4.5.48  | MIT OR Apache-2.0 | https://github.com/clap-rs/clap             |
| clap_derive   | 4.5.47  | MIT OR Apache-2.0 | https://github.com/clap-rs/clap             |
| clap_lex      | 0.7.5   | MIT OR Apache-2.0 | https://github.com/clap-rs/clap             |
| clap-markdown | 0.1.5   | MIT OR Apache-2.0 | https://github.com/ConnorGray/clap-markdown |
| console       | 0.15.11 | MIT               | https://github.com/console-rs/console       |

## Compression & Encoding

| Package   | Version | License           | Repository                                  |
| --------- | ------- | ----------------- | ------------------------------------------- |
| crc       | 3.3.0   | MIT OR Apache-2.0 | https://github.com/mrhooray/crc-rs.git      |
| crc-fast  | 1.6.0   | MIT OR Apache-2.0 | https://github.com/awesomized/crc-fast-rust |
| crc32fast | 1.5.0   | MIT OR Apache-2.0 | https://github.com/srijs/rust-crc32fast     |
| flate2    | 1.1.4   | MIT OR Apache-2.0 | https://github.com/rust-lang/flate2-rs      |

## Development & Testing

| Package        | Version | License        | Repository                                |
| -------------- | ------- | -------------- | ----------------------------------------- |
| assert_matches | 1.5.0   | MIT/Apache-2.0 | https://github.com/murarth/assert_matches |

## Utilities & Helpers

| Package                   | Version | License                              | Repository                                                      |
| ------------------------- | ------- | ------------------------------------ | --------------------------------------------------------------- |
| addr2line                 | 0.25.1  | Apache-2.0 OR MIT                    | https://github.com/gimli-rs/addr2line                           |
| adler2                    | 2.0.1   | 0BSD OR MIT OR Apache-2.0            | https://github.com/oyvindln/adler2                              |
| ahash                     | 0.8.12  | MIT OR Apache-2.0                    | https://github.com/tkaitchuck/ahash                             |
| allocator-api2            | 0.2.21  | MIT OR Apache-2.0                    | https://github.com/zakarumych/allocator-api2                    |
| android_system_properties | 0.1.5   | MIT/Apache-2.0                       | https://github.com/nical/android_system_properties              |
| anstream                  | 0.6.21  | MIT OR Apache-2.0                    | https://github.com/rust-cli/anstyle.git                         |
| anstyle                   | 1.0.13  | MIT OR Apache-2.0                    | https://github.com/rust-cli/anstyle.git                         |
| anstyle-parse             | 0.2.7   | MIT OR Apache-2.0                    | https://github.com/rust-cli/anstyle.git                         |
| anstyle-query             | 1.1.4   | MIT OR Apache-2.0                    | https://github.com/rust-cli/anstyle.git                         |
| anstyle-wincon            | 3.0.10  | MIT OR Apache-2.0                    | https://github.com/rust-cli/anstyle.git                         |
| anyhow                    | 1.0.100 | MIT OR Apache-2.0                    | https://github.com/dtolnay/anyhow                               |
| arc-swap                  | 1.7.1   | MIT OR Apache-2.0                    | https://github.com/vorner/arc-swap                              |
| atoi                      | 2.0.0   | MIT                                  | https://github.com/pacman82/atoi-rs                             |
| atomic-waker              | 1.1.2   | Apache-2.0 OR MIT                    | https://github.com/smol-rs/atomic-waker                         |
| autocfg                   | 1.5.0   | Apache-2.0 OR MIT                    | https://github.com/cuviper/autocfg                              |
| backoff                   | 0.4.0   | MIT/Apache-2.0                       | https://github.com/ihrwein/backoff                              |
| backtrace                 | 0.3.76  | MIT OR Apache-2.0                    | https://github.com/rust-lang/backtrace-rs                       |
| base16ct                  | 0.1.1   | Apache-2.0 OR MIT                    | https://github.com/RustCrypto/formats/tree/master/base16ct      |
| base64                    | 0.21.7  | MIT OR Apache-2.0                    | https://github.com/marshallpierce/rust-base64                   |
| base64                    | 0.22.1  | MIT OR Apache-2.0                    | https://github.com/marshallpierce/rust-base64                   |
| base64-simd               | 0.8.0   | MIT                                  | https://github.com/Nugine/simd                                  |
| base64ct                  | 1.8.0   | Apache-2.0 OR MIT                    | https://github.com/RustCrypto/formats                           |
| bitflags                  | 1.3.2   | MIT/Apache-2.0                       | https://github.com/bitflags/bitflags                            |
| bitflags                  | 2.9.4   | MIT OR Apache-2.0                    | https://github.com/bitflags/bitflags                            |
| bitpacking                | 0.9.2   | MIT                                  | https://github.com/quickwit-oss/bitpacking                      |
| block-buffer              | 0.10.4  | MIT OR Apache-2.0                    | https://github.com/RustCrypto/utils                             |
| bon                       | 3.8.1   | MIT OR Apache-2.0                    | https://github.com/elastio/bon                                  |
| bon-macros                | 3.8.1   | MIT OR Apache-2.0                    | https://github.com/elastio/bon                                  |
| build-print               | 1.0.1   | MIT                                  | https://github.com/sam0x17/build-print                          |
| bumpalo                   | 3.19.0  | MIT OR Apache-2.0                    | https://github.com/fitzgen/bumpalo                              |
| byteorder                 | 1.5.0   | Unlicense OR MIT                     | https://github.com/BurntSushi/byteorder                         |
| bytes                     | 1.10.1  | MIT                                  | https://github.com/tokio-rs/bytes                               |
| bytes-utils               | 0.1.4   | Apache-2.0/MIT                       | https://github.com/vorner/bytes-utils                           |
| cached                    | 0.49.3  | MIT                                  | https://github.com/jaemk/cached                                 |
| cached_proc_macro         | 0.20.0  | MIT                                  | https://github.com/jaemk/cached                                 |
| cached_proc_macro_types   | 0.1.1   | MIT                                  | https://github.com/jaemk/cached                                 |
| cc                        | 1.2.51  | MIT OR Apache-2.0                    | https://github.com/rust-lang/cc-rs                              |
| census                    | 0.4.2   | MIT                                  | https://github.com/quickwit-inc/census                          |
| cfg_aliases               | 0.2.1   | MIT                                  | https://github.com/katharostech/cfg_aliases                     |
| cfg-if                    | 1.0.3   | MIT OR Apache-2.0                    | https://github.com/rust-lang/cfg-if                             |
| chrono                    | 0.4.42  | MIT OR Apache-2.0                    | https://github.com/chronotope/chrono                            |
| cmake                     | 0.1.57  | MIT OR Apache-2.0                    | https://github.com/rust-lang/cmake-rs                           |
| color-eyre                | 0.6.5   | MIT OR Apache-2.0                    | https://github.com/eyre-rs/eyre                                 |
| color-spantrace           | 0.3.0   | MIT OR Apache-2.0                    | https://github.com/eyre-rs/eyre                                 |
| colorchoice               | 1.0.4   | MIT OR Apache-2.0                    | https://github.com/rust-cli/anstyle.git                         |
| const-oid                 | 0.9.6   | Apache-2.0 OR MIT                    | https://github.com/RustCrypto/formats/tree/master/const-oid     |
| core-foundation           | 0.10.1  | MIT OR Apache-2.0                    | https://github.com/servo/core-foundation-rs                     |
| core-foundation           | 0.9.4   | MIT OR Apache-2.0                    | https://github.com/servo/core-foundation-rs                     |
| core-foundation-sys       | 0.8.7   | MIT OR Apache-2.0                    | https://github.com/servo/core-foundation-rs                     |
| cpufeatures               | 0.2.17  | MIT OR Apache-2.0                    | https://github.com/RustCrypto/utils                             |
| crossbeam-channel         | 0.5.15  | MIT OR Apache-2.0                    | https://github.com/crossbeam-rs/crossbeam                       |
| crossbeam-deque           | 0.8.6   | MIT OR Apache-2.0                    | https://github.com/crossbeam-rs/crossbeam                       |
| crossbeam-epoch           | 0.9.18  | MIT OR Apache-2.0                    | https://github.com/crossbeam-rs/crossbeam                       |
| crossbeam-queue           | 0.3.12  | MIT OR Apache-2.0                    | https://github.com/crossbeam-rs/crossbeam                       |
| crossbeam-utils           | 0.8.21  | MIT OR Apache-2.0                    | https://github.com/crossbeam-rs/crossbeam                       |
| crunchy                   | 0.2.4   | MIT                                  | https://github.com/eira-fransham/crunchy                        |
| darling                   | 0.14.4  | MIT                                  | https://github.com/TedDriggs/darling                            |
| darling                   | 0.20.11 | MIT                                  | https://github.com/TedDriggs/darling                            |
| darling                   | 0.21.3  | MIT                                  | https://github.com/TedDriggs/darling                            |
| darling_core              | 0.14.4  | MIT                                  | https://github.com/TedDriggs/darling                            |
| darling_core              | 0.20.11 | MIT                                  | https://github.com/TedDriggs/darling                            |
| darling_core              | 0.21.3  | MIT                                  | https://github.com/TedDriggs/darling                            |
| darling_macro             | 0.14.4  | MIT                                  | https://github.com/TedDriggs/darling                            |
| darling_macro             | 0.20.11 | MIT                                  | https://github.com/TedDriggs/darling                            |
| darling_macro             | 0.21.3  | MIT                                  | https://github.com/TedDriggs/darling                            |
| dashmap                   | 5.5.3   | MIT                                  | https://github.com/xacrimon/dashmap                             |
| data-encoding             | 2.9.0   | MIT                                  | https://github.com/ia0/data-encoding                            |
| der                       | 0.6.1   | Apache-2.0 OR MIT                    | https://github.com/RustCrypto/formats/tree/master/der           |
| der                       | 0.7.10  | Apache-2.0 OR MIT                    | https://github.com/RustCrypto/formats/tree/master/der           |
| deranged                  | 0.5.4   | MIT OR Apache-2.0                    | https://github.com/jhpratt/deranged                             |
| displaydoc                | 0.2.5   | MIT OR Apache-2.0                    | https://github.com/yaahc/displaydoc                             |
| dotenvy                   | 0.15.7  | MIT                                  | https://github.com/allan2/dotenvy                               |
| downcast-rs               | 1.2.1   | MIT/Apache-2.0                       | https://github.com/marcianx/downcast-rs                         |
| dunce                     | 1.0.5   | CC0-1.0 OR MIT-0 OR Apache-2.0       | https://gitlab.com/kornelski/dunce                              |
| dyn-clone                 | 1.0.20  | MIT OR Apache-2.0                    | https://github.com/dtolnay/dyn-clone                            |
| ecdsa                     | 0.14.8  | Apache-2.0 OR MIT                    | https://github.com/RustCrypto/signatures/tree/master/ecdsa      |
| either                    | 1.15.0  | MIT OR Apache-2.0                    | https://github.com/rayon-rs/either                              |
| elliptic-curve            | 0.12.3  | Apache-2.0 OR MIT                    | https://github.com/RustCrypto/traits/tree/master/elliptic-curve |
| encoding_rs               | 0.8.35  | (Apache-2.0 OR MIT) AND BSD-3-Clause | https://github.com/hsivonen/encoding_rs                         |
| equivalent                | 1.0.2   | Apache-2.0 OR MIT                    | https://github.com/indexmap-rs/equivalent                       |
| errno                     | 0.3.14  | MIT OR Apache-2.0                    | https://github.com/lambda-fairy/rust-errno                      |
| etcetera                  | 0.8.0   | MIT OR Apache-2.0                    | https://github.com/lunacookies/etcetera                         |
| event-listener            | 2.5.3   | Apache-2.0 OR MIT                    | https://github.com/smol-rs/event-listener                       |
| eventsource-stream        | 0.2.3   | MIT OR Apache-2.0                    | https://github.com/jpopesculian/eventsource-stream              |
| eyre                      | 0.6.12  | MIT OR Apache-2.0                    | https://github.com/eyre-rs/eyre                                 |
| fastdivide                | 0.4.2   | zlib-acknowledgement OR MIT          | https://github.com/fulmicoton/fastdivide                        |
| fastrand                  | 2.3.0   | Apache-2.0 OR MIT                    | https://github.com/smol-rs/fastrand                             |
| ff                        | 0.12.1  | MIT/Apache-2.0                       | https://github.com/zkcrypto/ff                                  |
| fid-rs                    | 0.1.1   | MIT OR Apache-2.0                    | https://github.com/laysakura/fid-rs                             |
| find-msvc-tools           | 0.1.6   | MIT OR Apache-2.0                    | https://github.com/rust-lang/cc-rs                              |
| flume                     | 0.11.1  | Apache-2.0/MIT                       | https://github.com/zesterer/flume                               |
| fnv                       | 1.0.7   | Apache-2.0 / MIT                     | https://github.com/servo/rust-fnv                               |
| foldhash                  | 0.1.5   | Zlib                                 | https://github.com/orlp/foldhash                                |
| foreign-types             | 0.3.2   | MIT/Apache-2.0                       | https://github.com/sfackler/foreign-types                       |
| form_urlencoded           | 1.2.2   | MIT OR Apache-2.0                    | https://github.com/servo/rust-url                               |
| fs_extra                  | 1.3.0   | MIT                                  | https://github.com/webdesus/fs_extra                            |
| fs4                       | 0.8.4   | MIT OR Apache-2.0                    | https://github.com/al8n/fs4-rs                                  |
| fslock                    | 0.2.1   | MIT                                  | https://github.com/brunoczim/fslock                             |
| generator                 | 0.7.5   | MIT/Apache-2.0                       | https://github.com/Xudong-Huang/generator-rs.git                |
| generic-array             | 0.14.7  | MIT                                  | https://github.com/fizyk20/generic-array.git                    |
| getrandom                 | 0.2.16  | MIT OR Apache-2.0                    | https://github.com/rust-random/getrandom                        |
| getrandom                 | 0.3.3   | MIT OR Apache-2.0                    | https://github.com/rust-random/getrandom                        |
| gimli                     | 0.32.3  | MIT OR Apache-2.0                    | https://github.com/gimli-rs/gimli                               |
| glob                      | 0.3.3   | MIT OR Apache-2.0                    | https://github.com/rust-lang/glob                               |
| group                     | 0.12.1  | MIT/Apache-2.0                       | https://github.com/zkcrypto/group                               |
| h2                        | 0.3.27  | MIT                                  | https://github.com/hyperium/h2                                  |
| h2                        | 0.4.12  | MIT                                  | https://github.com/hyperium/h2                                  |
| hashbrown                 | 0.12.3  | MIT OR Apache-2.0                    | https://github.com/rust-lang/hashbrown                          |
| hashbrown                 | 0.14.5  | MIT OR Apache-2.0                    | https://github.com/rust-lang/hashbrown                          |
| hashbrown                 | 0.15.5  | MIT OR Apache-2.0                    | https://github.com/rust-lang/hashbrown                          |
| hashbrown                 | 0.16.0  | MIT OR Apache-2.0                    | https://github.com/rust-lang/hashbrown                          |
| hashlink                  | 0.8.4   | MIT OR Apache-2.0                    | https://github.com/kyren/hashlink                               |
| headers                   | 0.4.1   | MIT                                  | https://github.com/hyperium/headers                             |
| headers-core              | 0.3.0   | MIT                                  | https://github.com/hyperium/headers                             |
| heck                      | 0.4.1   | MIT OR Apache-2.0                    | https://github.com/withoutboats/heck                            |
| heck                      | 0.5.0   | MIT OR Apache-2.0                    | https://github.com/withoutboats/heck                            |
| hermit-abi                | 0.5.2   | MIT OR Apache-2.0                    | https://github.com/hermit-os/hermit-rs                          |
| hex                       | 0.4.3   | MIT OR Apache-2.0                    | https://github.com/KokaKiwi/rust-hex                            |
| hkdf                      | 0.12.4  | MIT OR Apache-2.0                    | https://github.com/RustCrypto/KDFs/                             |
| home                      | 0.5.11  | MIT OR Apache-2.0                    | https://github.com/rust-lang/cargo                              |
| htmlescape                | 0.3.1   | Apache-2.0 / MIT / MPL-2.0           | https://github.com/veddan/rust-htmlescape                       |
| humantime                 | 2.3.0   | MIT OR Apache-2.0                    | https://github.com/chronotope/humantime                         |
| iana-time-zone            | 0.1.64  | MIT OR Apache-2.0                    | https://github.com/strawlab/iana-time-zone                      |
| iana-time-zone-haiku      | 0.1.2   | MIT OR Apache-2.0                    | https://github.com/strawlab/iana-time-zone                      |
| icu_collections           | 2.0.0   | Unicode-3.0                          | https://github.com/unicode-org/icu4x                            |
| icu_locale_core           | 2.0.0   | Unicode-3.0                          | https://github.com/unicode-org/icu4x                            |
| icu_normalizer            | 2.0.0   | Unicode-3.0                          | https://github.com/unicode-org/icu4x                            |
| icu_normalizer_data       | 2.0.0   | Unicode-3.0                          | https://github.com/unicode-org/icu4x                            |
| icu_properties            | 2.0.1   | Unicode-3.0                          | https://github.com/unicode-org/icu4x                            |
| icu_properties_data       | 2.0.1   | Unicode-3.0                          | https://github.com/unicode-org/icu4x                            |
| icu_provider              | 2.0.0   | Unicode-3.0                          | https://github.com/unicode-org/icu4x                            |
| ident_case                | 1.0.1   | MIT/Apache-2.0                       | https://github.com/TedDriggs/ident_case                         |
| idna                      | 1.1.0   | MIT OR Apache-2.0                    | https://github.com/servo/rust-url/                              |
| idna_adapter              | 1.2.1   | Apache-2.0 OR MIT                    | https://github.com/hsivonen/idna_adapter                        |
| indenter                  | 0.3.4   | MIT OR Apache-2.0                    | https://                                                        |

---

## Compliance Notes

### Attribution Requirements

All dependencies with attribution requirements (MIT, Apache, BSD, etc.) are
properly attributed in this document. When redistributing or modifying shaide
server, you must maintain this attribution.

### Dual Licensing

Many Rust crates are dual-licensed under MIT OR Apache-2.0. You may choose which
license to comply with for each package.

### Patent Grants

Dependencies under Apache License 2.0 include explicit patent grants, protecting
users from patent claims related to the software.

### No Warranty

All open source software included in shaide server is provided "as is" without
warranty of any kind, express or implied.
