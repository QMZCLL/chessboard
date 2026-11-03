# 第三方许可清单

由 `node scripts/third-party-licenses.mjs` 生成，不要手工修改；CI 检查它是最新的，并且每一项的许可都在允许的列表中。

## 随安装包分发的程序与数据

| 名称 | 许可 | 说明 |
| --- | --- | --- |
| Pikafish 2026-09-06（引擎） | GPL-3.0 | 许可全文：libs/pikafish/Copying.txt；作者：libs/pikafish/AUTHORS；对应源码：https://github.com/official-pikafish/Pikafish/releases/tag/Pikafish-2026-09-06 |
| pikafish.nnue（网络文件） | 见 libs/pikafish/NNUE-License.md | 仅限合法用途，未经许可不得商用 |

## 桌面程序依赖的 Rust crate（582 个）

| 名称 | 版本 | 许可 |
| --- | --- | --- |
| addr2line | 0.24.2 | Apache-2.0 OR MIT |
| adler2 | 2.0.0 | 0BSD OR MIT OR Apache-2.0 |
| aho-corasick | 1.1.3 | Unlicense OR MIT |
| alloc-no-stdlib | 2.0.4 | BSD-3-Clause |
| alloc-stdlib | 0.2.2 | BSD-3-Clause |
| android-tzdata | 0.1.1 | MIT OR Apache-2.0 |
| android_system_properties | 0.1.5 | MIT/Apache-2.0 |
| anyhow | 1.0.98 | MIT OR Apache-2.0 |
| ashpd | 0.11.1 | MIT |
| async-broadcast | 0.7.2 | MIT OR Apache-2.0 |
| async-channel | 2.3.1 | Apache-2.0 OR MIT |
| async-executor | 1.13.2 | Apache-2.0 OR MIT |
| async-io | 2.4.0 | Apache-2.0 OR MIT |
| async-lock | 3.4.0 | Apache-2.0 OR MIT |
| async-process | 2.3.0 | Apache-2.0 OR MIT |
| async-recursion | 1.1.1 | MIT OR Apache-2.0 |
| async-signal | 0.2.10 | Apache-2.0 OR MIT |
| async-task | 4.7.1 | Apache-2.0 OR MIT |
| async-trait | 0.1.88 | MIT OR Apache-2.0 |
| atk | 0.18.2 | MIT |
| atk-sys | 0.18.2 | MIT |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT |
| backtrace | 0.3.74 | MIT OR Apache-2.0 |
| base64 | 0.21.7 | MIT OR Apache-2.0 |
| base64 | 0.22.1 | MIT OR Apache-2.0 |
| bitflags | 1.3.2 | MIT/Apache-2.0 |
| bitflags | 2.9.0 | MIT OR Apache-2.0 |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 |
| block2 | 0.5.1 | MIT |
| block2 | 0.6.1 | MIT |
| blocking | 1.6.1 | Apache-2.0 OR MIT |
| brotli | 7.0.0 | BSD-3-Clause AND MIT |
| brotli-decompressor | 4.0.3 | BSD-3-Clause/MIT |
| bumpalo | 3.17.0 | MIT OR Apache-2.0 |
| bytemuck | 1.23.0 | Zlib OR Apache-2.0 OR MIT |
| byteorder | 1.5.0 | Unlicense OR MIT |
| byteorder-lite | 0.1.0 | Unlicense OR MIT |
| bytes | 1.10.1 | MIT |
| cairo-rs | 0.18.5 | MIT |
| cairo-sys-rs | 0.18.2 | MIT |
| camino | 1.1.9 | MIT OR Apache-2.0 |
| cargo-platform | 0.1.9 | MIT OR Apache-2.0 |
| cargo_metadata | 0.19.2 | MIT |
| cesu8 | 1.1.0 | Apache-2.0/MIT |
| cfb | 0.7.3 | MIT |
| cfg-if | 1.0.0 | MIT/Apache-2.0 |
| chacha20 | 0.10.2 | MIT OR Apache-2.0 |
| chrono | 0.4.41 | MIT OR Apache-2.0 |
| color_quant | 1.1.0 | MIT |
| combine | 4.6.7 | MIT |
| concurrent-queue | 2.5.0 | Apache-2.0 OR MIT |
| convert_case | 0.4.0 | MIT |
| convert_case | 0.6.0 | MIT |
| cookie | 0.18.1 | MIT OR Apache-2.0 |
| cookie-factory | 0.3.3 | MIT |
| core-foundation | 0.10.0 | MIT OR Apache-2.0 |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 |
| core-graphics | 0.24.0 | MIT OR Apache-2.0 |
| core-graphics-types | 0.2.0 | MIT OR Apache-2.0 |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 |
| crc32fast | 1.4.2 | MIT OR Apache-2.0 |
| crossbeam-channel | 0.5.15 | MIT OR Apache-2.0 |
| crossbeam-utils | 0.8.21 | MIT OR Apache-2.0 |
| crunchy | 0.2.3 | MIT |
| crypto-common | 0.1.6 | MIT OR Apache-2.0 |
| cssparser | 0.27.2 | MPL-2.0 |
| cssparser-macros | 0.6.1 | MPL-2.0 |
| ctor | 0.2.9 | Apache-2.0 OR MIT |
| darling | 0.20.11 | MIT |
| darling_core | 0.20.11 | MIT |
| darling_macro | 0.20.11 | MIT |
| deranged | 0.4.0 | MIT OR Apache-2.0 |
| derive_more | 0.99.20 | MIT |
| digest | 0.10.7 | MIT OR Apache-2.0 |
| dirs | 6.0.0 | MIT OR Apache-2.0 |
| dirs-sys | 0.5.0 | MIT OR Apache-2.0 |
| dispatch | 0.2.0 | MIT |
| dispatch2 | 0.3.0 | Zlib OR Apache-2.0 OR MIT |
| displaydoc | 0.2.5 | MIT OR Apache-2.0 |
| dlib | 0.5.3 | MIT |
| dlopen2 | 0.7.0 | MIT |
| dlopen2_derive | 0.4.0 | MIT |
| downcast-rs | 1.2.1 | MIT/Apache-2.0 |
| dpi | 0.1.2 | Apache-2.0 AND MIT |
| dtoa | 1.0.10 | MIT OR Apache-2.0 |
| dtoa-short | 0.3.5 | MPL-2.0 |
| dunce | 1.0.5 | CC0-1.0 OR MIT-0 OR Apache-2.0 |
| dyn-clone | 1.0.19 | MIT OR Apache-2.0 |
| embed_plist | 1.2.2 | MIT OR Apache-2.0 |
| endi | 1.1.0 | MIT |
| enumflags2 | 0.7.11 | MIT OR Apache-2.0 |
| enumflags2_derive | 0.7.11 | MIT OR Apache-2.0 |
| equivalent | 1.0.2 | Apache-2.0 OR MIT |
| erased-serde | 0.4.6 | MIT OR Apache-2.0 |
| errno | 0.3.11 | MIT OR Apache-2.0 |
| event-listener | 5.4.0 | Apache-2.0 OR MIT |
| event-listener-strategy | 0.5.4 | Apache-2.0 OR MIT |
| fastrand | 2.3.0 | Apache-2.0 OR MIT |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 |
| field-offset | 0.3.6 | MIT OR Apache-2.0 |
| flate2 | 1.1.1 | MIT OR Apache-2.0 |
| fnv | 1.0.7 | Apache-2.0 / MIT |
| foreign-types | 0.3.2 | MIT/Apache-2.0 |
| foreign-types | 0.5.0 | MIT/Apache-2.0 |
| foreign-types-macros | 0.2.3 | MIT/Apache-2.0 |
| foreign-types-shared | 0.1.1 | MIT/Apache-2.0 |
| foreign-types-shared | 0.3.1 | MIT/Apache-2.0 |
| form_urlencoded | 1.2.1 | MIT OR Apache-2.0 |
| futf | 0.1.5 | MIT / Apache-2.0 |
| futures | 0.3.31 | MIT OR Apache-2.0 |
| futures-channel | 0.3.31 | MIT OR Apache-2.0 |
| futures-core | 0.3.31 | MIT OR Apache-2.0 |
| futures-executor | 0.3.31 | MIT OR Apache-2.0 |
| futures-io | 0.3.31 | MIT OR Apache-2.0 |
| futures-lite | 2.6.0 | Apache-2.0 OR MIT |
| futures-macro | 0.3.31 | MIT OR Apache-2.0 |
| futures-sink | 0.3.31 | MIT OR Apache-2.0 |
| futures-task | 0.3.31 | MIT OR Apache-2.0 |
| futures-util | 0.3.31 | MIT OR Apache-2.0 |
| fxhash | 0.2.1 | Apache-2.0/MIT |
| gdk | 0.18.2 | MIT |
| gdk-pixbuf | 0.18.5 | MIT |
| gdk-pixbuf-sys | 0.18.0 | MIT |
| gdk-sys | 0.18.2 | MIT |
| gdkwayland-sys | 0.18.2 | MIT |
| gdkx11 | 0.18.2 | MIT |
| gdkx11-sys | 0.18.2 | MIT |
| generic-array | 0.14.7 | MIT |
| getrandom | 0.1.16 | MIT OR Apache-2.0 |
| getrandom | 0.2.16 | MIT OR Apache-2.0 |
| getrandom | 0.3.2 | MIT OR Apache-2.0 |
| getrandom | 0.4.3 | MIT OR Apache-2.0 |
| gimli | 0.31.1 | MIT OR Apache-2.0 |
| gio | 0.18.4 | MIT |
| gio-sys | 0.18.1 | MIT |
| glib | 0.18.5 | MIT |
| glib-macros | 0.18.5 | MIT |
| glib-sys | 0.18.1 | MIT |
| glob | 0.3.2 | MIT OR Apache-2.0 |
| gobject-sys | 0.18.0 | MIT |
| gtk | 0.18.2 | MIT |
| gtk-sys | 0.18.2 | MIT |
| gtk3-macros | 0.18.2 | MIT |
| half | 2.6.0 | MIT OR Apache-2.0 |
| hashbrown | 0.12.3 | MIT OR Apache-2.0 |
| hashbrown | 0.15.3 | MIT OR Apache-2.0 |
| heck | 0.4.1 | MIT OR Apache-2.0 |
| heck | 0.5.0 | MIT OR Apache-2.0 |
| hermit-abi | 0.4.0 | MIT OR Apache-2.0 |
| hex | 0.4.3 | MIT OR Apache-2.0 |
| html5ever | 0.26.0 | MIT OR Apache-2.0 |
| http | 1.3.1 | MIT OR Apache-2.0 |
| http-body | 1.0.1 | MIT |
| http-body-util | 0.1.3 | MIT |
| httparse | 1.10.1 | MIT OR Apache-2.0 |
| hyper | 1.6.0 | MIT |
| hyper-rustls | 0.27.10 | Apache-2.0 OR ISC OR MIT |
| hyper-tls | 0.6.0 | MIT/Apache-2.0 |
| hyper-util | 0.1.11 | MIT |
| iana-time-zone | 0.1.63 | MIT OR Apache-2.0 |
| iana-time-zone-haiku | 0.1.2 | MIT OR Apache-2.0 |
| ico | 0.4.0 | MIT |
| icu_collections | 1.5.0 | Unicode-3.0 |
| icu_locid | 1.5.0 | Unicode-3.0 |
| icu_locid_transform | 1.5.0 | Unicode-3.0 |
| icu_locid_transform_data | 1.5.1 | Unicode-3.0 |
| icu_normalizer | 1.5.0 | Unicode-3.0 |
| icu_normalizer_data | 1.5.1 | Unicode-3.0 |
| icu_properties | 1.5.1 | Unicode-3.0 |
| icu_properties_data | 1.5.1 | Unicode-3.0 |
| icu_provider | 1.5.0 | Unicode-3.0 |
| icu_provider_macros | 1.5.0 | Unicode-3.0 |
| ident_case | 1.0.1 | MIT/Apache-2.0 |
| idna | 1.0.3 | MIT OR Apache-2.0 |
| idna_adapter | 1.2.0 | Apache-2.0 OR MIT |
| image | 0.24.9 | MIT OR Apache-2.0 |
| image | 0.25.6 | MIT OR Apache-2.0 |
| indexmap | 1.9.3 | Apache-2.0 OR MIT |
| indexmap | 2.9.0 | Apache-2.0 OR MIT |
| infer | 0.19.0 | MIT |
| ipnet | 2.11.0 | MIT OR Apache-2.0 |
| is-docker | 0.2.0 | MIT |
| is-wsl | 0.4.0 | MIT |
| itoa | 0.4.8 | MIT OR Apache-2.0 |
| itoa | 1.0.15 | MIT OR Apache-2.0 |
| javascriptcore-rs | 1.1.2 | MIT |
| javascriptcore-rs-sys | 1.1.1 | MIT |
| jni | 0.21.1 | MIT/Apache-2.0 |
| jni-sys | 0.3.0 | MIT/Apache-2.0 |
| js-sys | 0.3.77 | MIT OR Apache-2.0 |
| json-patch | 3.0.1 | MIT/Apache-2.0 |
| jsonptr | 0.6.3 | MIT OR Apache-2.0 |
| keyboard-types | 0.7.0 | MIT OR Apache-2.0 |
| kuchikiki | 0.8.2 | MIT |
| lazy_static | 1.5.0 | MIT OR Apache-2.0 |
| libappindicator | 0.9.0 | Apache-2.0 OR MIT |
| libappindicator-sys | 0.9.0 | Apache-2.0 OR MIT |
| libc | 0.2.172 | MIT OR Apache-2.0 |
| libloading | 0.7.4 | ISC |
| libloading | 0.8.6 | ISC |
| libredox | 0.1.3 | MIT |
| libspa | 0.8.0 | MIT |
| libspa-sys | 0.8.0 | MIT |
| libwayshot | 0.3.0 | BSD-2-Clause |
| linux-raw-sys | 0.4.15 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| linux-raw-sys | 0.9.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| litemap | 0.7.5 | Unicode-3.0 |
| lock_api | 0.4.12 | MIT OR Apache-2.0 |
| log | 0.4.27 | MIT OR Apache-2.0 |
| lru-slab | 0.1.3 | MIT OR Apache-2.0 OR Zlib |
| mac | 0.1.1 | MIT/Apache-2.0 |
| markup5ever | 0.11.0 | MIT OR Apache-2.0 |
| matchers | 0.1.0 | MIT |
| matches | 0.1.10 | MIT |
| matrixmultiply | 0.3.9 | MIT/Apache-2.0 |
| memchr | 2.7.4 | Unlicense OR MIT |
| memmap2 | 0.9.5 | MIT OR Apache-2.0 |
| memoffset | 0.9.1 | MIT |
| mime | 0.3.17 | MIT OR Apache-2.0 |
| minimal-lexical | 0.2.1 | MIT/Apache-2.0 |
| miniz_oxide | 0.8.8 | MIT OR Zlib OR Apache-2.0 |
| mio | 1.0.3 | MIT |
| muda | 0.16.1 | Apache-2.0 OR MIT |
| native-tls | 0.2.18 | MIT OR Apache-2.0 |
| ndarray | 0.16.1 | MIT OR Apache-2.0 |
| ndk | 0.9.0 | MIT OR Apache-2.0 |
| ndk-context | 0.1.1 | MIT OR Apache-2.0 |
| ndk-sys | 0.6.0+11769913 | MIT OR Apache-2.0 |
| new_debug_unreachable | 1.0.6 | MIT |
| nix | 0.27.1 | MIT |
| nix | 0.29.0 | MIT |
| nodrop | 0.1.14 | MIT/Apache-2.0 |
| nom | 7.1.3 | MIT |
| nu-ansi-term | 0.46.0 | MIT |
| num-complex | 0.4.6 | MIT OR Apache-2.0 |
| num-conv | 0.1.0 | MIT OR Apache-2.0 |
| num-integer | 0.1.46 | MIT OR Apache-2.0 |
| num-traits | 0.2.19 | MIT OR Apache-2.0 |
| num_enum | 0.7.3 | BSD-3-Clause OR MIT OR Apache-2.0 |
| num_enum_derive | 0.7.3 | BSD-3-Clause OR MIT OR Apache-2.0 |
| num_threads | 0.1.7 | MIT OR Apache-2.0 |
| objc-sys | 0.3.5 | MIT |
| objc2 | 0.5.2 | MIT |
| objc2 | 0.6.1 | MIT |
| objc2-app-kit | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-av-foundation | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-avf-audio | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-cloud-kit | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-core-audio | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-core-audio-types | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-core-data | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-core-foundation | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-core-graphics | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-core-image | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-core-media | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-core-video | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-encode | 4.1.0 | MIT |
| objc2-exception-helper | 0.1.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-foundation | 0.2.2 | MIT |
| objc2-foundation | 0.3.1 | MIT |
| objc2-io-surface | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-metal | 0.2.2 | MIT |
| objc2-metal | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-quartz-core | 0.2.2 | MIT |
| objc2-quartz-core | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-ui-kit | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| objc2-web-kit | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| object | 0.36.7 | Apache-2.0 OR MIT |
| once_cell | 1.21.3 | MIT OR Apache-2.0 |
| open | 5.3.2 | MIT |
| openssl | 0.10.81 | Apache-2.0 |
| openssl-macros | 0.1.1 | MIT/Apache-2.0 |
| openssl-probe | 0.2.1 | MIT OR Apache-2.0 |
| openssl-sys | 0.9.117 | MIT |
| option-ext | 0.2.0 | MPL-2.0 |
| ordered-stream | 0.2.0 | MIT OR Apache-2.0 |
| ort | 2.0.0-rc.9 | MIT OR Apache-2.0 |
| ort-sys | 2.0.0-rc.9 | MIT OR Apache-2.0 |
| overload | 0.1.1 | MIT |
| pango | 0.18.3 | MIT |
| pango-sys | 0.18.0 | MIT |
| parking | 2.2.1 | Apache-2.0 OR MIT |
| parking_lot | 0.12.3 | MIT OR Apache-2.0 |
| parking_lot_core | 0.9.10 | MIT OR Apache-2.0 |
| pathdiff | 0.2.3 | MIT/Apache-2.0 |
| percent-encoding | 2.3.1 | MIT OR Apache-2.0 |
| phf | 0.10.1 | MIT |
| phf | 0.11.3 | MIT |
| phf | 0.8.0 | MIT |
| phf_generator | 0.11.3 | MIT |
| phf_generator | 0.8.0 | MIT |
| phf_macros | 0.11.3 | MIT |
| phf_macros | 0.8.0 | MIT |
| phf_shared | 0.10.0 | MIT |
| phf_shared | 0.11.3 | MIT |
| phf_shared | 0.8.0 | MIT |
| pin-project-lite | 0.2.16 | Apache-2.0 OR MIT |
| pin-utils | 0.1.0 | MIT OR Apache-2.0 |
| piper | 0.2.4 | MIT OR Apache-2.0 |
| pipewire | 0.8.0 | MIT |
| pipewire-sys | 0.8.0 | MIT |
| plist | 1.7.1 | MIT |
| png | 0.17.16 | MIT OR Apache-2.0 |
| polling | 3.7.4 | Apache-2.0 OR MIT |
| portable-atomic | 1.11.0 | Apache-2.0 OR MIT |
| portable-atomic-util | 0.2.4 | Apache-2.0 OR MIT |
| powerfmt | 0.2.0 | MIT OR Apache-2.0 |
| ppv-lite86 | 0.2.21 | MIT OR Apache-2.0 |
| precomputed-hash | 0.1.1 | MIT |
| proc-macro-crate | 1.3.1 | MIT OR Apache-2.0 |
| proc-macro-crate | 2.0.0 | MIT OR Apache-2.0 |
| proc-macro-crate | 3.3.0 | MIT OR Apache-2.0 |
| proc-macro-error | 1.0.4 | MIT OR Apache-2.0 |
| proc-macro-error-attr | 1.0.4 | MIT OR Apache-2.0 |
| proc-macro-hack | 0.5.20+deprecated | MIT OR Apache-2.0 |
| proc-macro2 | 1.0.95 | MIT OR Apache-2.0 |
| quick-xml | 0.32.0 | MIT |
| quick-xml | 0.37.5 | MIT |
| quinn | 0.11.12 | MIT OR Apache-2.0 |
| quinn-proto | 0.11.19 | MIT OR Apache-2.0 |
| quinn-udp | 0.5.16 | MIT OR Apache-2.0 |
| quote | 1.0.40 | MIT OR Apache-2.0 |
| r-efi | 5.2.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| rand | 0.10.3 | MIT OR Apache-2.0 |
| rand | 0.7.3 | MIT OR Apache-2.0 |
| rand | 0.8.5 | MIT OR Apache-2.0 |
| rand | 0.9.1 | MIT OR Apache-2.0 |
| rand_chacha | 0.2.2 | MIT OR Apache-2.0 |
| rand_chacha | 0.3.1 | MIT OR Apache-2.0 |
| rand_chacha | 0.9.0 | MIT OR Apache-2.0 |
| rand_core | 0.10.1 | MIT OR Apache-2.0 |
| rand_core | 0.5.1 | MIT OR Apache-2.0 |
| rand_core | 0.6.4 | MIT OR Apache-2.0 |
| rand_core | 0.9.3 | MIT OR Apache-2.0 |
| rand_hc | 0.2.0 | MIT/Apache-2.0 |
| rand_pcg | 0.10.2 | MIT OR Apache-2.0 |
| rand_pcg | 0.2.1 | MIT OR Apache-2.0 |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib |
| rawpointer | 0.2.1 | MIT/Apache-2.0 |
| redb | 2.6.4 | MIT OR Apache-2.0 |
| redox_syscall | 0.5.12 | MIT |
| redox_users | 0.5.0 | MIT |
| regex | 1.11.1 | MIT OR Apache-2.0 |
| regex-automata | 0.1.10 | Unlicense/MIT |
| regex-automata | 0.4.9 | MIT OR Apache-2.0 |
| regex-syntax | 0.6.29 | MIT OR Apache-2.0 |
| regex-syntax | 0.8.5 | MIT OR Apache-2.0 |
| reqwest | 0.12.15 | MIT OR Apache-2.0 |
| rfd | 0.15.4 | MIT |
| ring | 0.17.14 | Apache-2.0 AND ISC |
| rustc-demangle | 0.1.24 | MIT/Apache-2.0 |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT |
| rustix | 0.38.44 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| rustix | 1.0.7 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| rustls | 0.23.26 | Apache-2.0 OR ISC OR MIT |
| rustls-pemfile | 2.2.0 | Apache-2.0 OR ISC OR MIT |
| rustls-pki-types | 1.11.0 | MIT OR Apache-2.0 |
| rustls-webpki | 0.103.1 | ISC |
| rustversion | 1.0.20 | MIT OR Apache-2.0 |
| ryu | 1.0.20 | Apache-2.0 OR BSL-1.0 |
| same-file | 1.0.6 | Unlicense/MIT |
| schannel | 0.1.30 | MIT |
| schemars | 0.8.22 | MIT |
| schemars_derive | 0.8.22 | MIT |
| scoped-tls | 1.0.1 | MIT/Apache-2.0 |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 |
| security-framework | 3.6.0 | MIT OR Apache-2.0 |
| security-framework-sys | 2.17.0 | MIT OR Apache-2.0 |
| selectors | 0.22.0 | MPL-2.0 |
| semver | 1.0.26 | MIT OR Apache-2.0 |
| serde | 1.0.219 | MIT OR Apache-2.0 |
| serde-untagged | 0.1.7 | MIT OR Apache-2.0 |
| serde_derive | 1.0.219 | MIT OR Apache-2.0 |
| serde_derive_internals | 0.29.1 | MIT OR Apache-2.0 |
| serde_json | 1.0.140 | MIT OR Apache-2.0 |
| serde_repr | 0.1.20 | MIT OR Apache-2.0 |
| serde_spanned | 0.6.8 | MIT OR Apache-2.0 |
| serde_urlencoded | 0.7.1 | MIT/Apache-2.0 |
| serde_with | 3.12.0 | MIT OR Apache-2.0 |
| serde_with_macros | 3.12.0 | MIT OR Apache-2.0 |
| serialize-to-javascript | 0.1.1 | MIT OR Apache-2.0 |
| serialize-to-javascript-impl | 0.1.1 | MIT OR Apache-2.0 |
| servo_arc | 0.1.1 | MIT/Apache-2.0 |
| sha2 | 0.10.9 | MIT OR Apache-2.0 |
| sharded-slab | 0.1.7 | MIT |
| signal-hook-registry | 1.4.5 | Apache-2.0/MIT |
| simd-adler32 | 0.3.7 | MIT |
| siphasher | 0.3.11 | MIT/Apache-2.0 |
| siphasher | 1.0.1 | MIT/Apache-2.0 |
| slab | 0.4.9 | MIT |
| smallvec | 1.15.0 | MIT OR Apache-2.0 |
| socket2 | 0.5.9 | MIT OR Apache-2.0 |
| softbuffer | 0.4.6 | MIT OR Apache-2.0 |
| soup3 | 0.5.0 | MIT |
| soup3-sys | 0.5.0 | MIT |
| stable_deref_trait | 1.2.0 | MIT/Apache-2.0 |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 |
| string_cache | 0.8.9 | MIT OR Apache-2.0 |
| strsim | 0.11.1 | MIT |
| subtle | 2.6.1 | BSD-3-Clause |
| swift-rs | 1.0.7 | MIT OR Apache-2.0 |
| syn | 1.0.109 | MIT OR Apache-2.0 |
| syn | 2.0.101 | MIT OR Apache-2.0 |
| sync_wrapper | 1.0.2 | Apache-2.0 |
| synstructure | 0.13.2 | MIT |
| tao | 0.33.0 | Apache-2.0 |
| tao-macros | 0.1.3 | MIT OR Apache-2.0 |
| tauri | 2.5.1 | Apache-2.0 OR MIT |
| tauri-codegen | 2.2.0 | Apache-2.0 OR MIT |
| tauri-macros | 2.2.0 | Apache-2.0 OR MIT |
| tauri-plugin-dialog | 2.2.2 | Apache-2.0 OR MIT |
| tauri-plugin-fs | 2.3.0 | Apache-2.0 OR MIT |
| tauri-plugin-opener | 2.2.6 | Apache-2.0 OR MIT |
| tauri-runtime | 2.6.0 | Apache-2.0 OR MIT |
| tauri-runtime-wry | 2.6.0 | Apache-2.0 OR MIT |
| tauri-utils | 2.4.0 | Apache-2.0 OR MIT |
| tempfile | 3.19.1 | MIT OR Apache-2.0 |
| tendril | 0.4.3 | MIT/Apache-2.0 |
| termcolor | 1.4.1 | Unlicense OR MIT |
| thin-slice | 0.1.1 | MPL-2.0 |
| thiserror | 1.0.69 | MIT OR Apache-2.0 |
| thiserror | 2.0.12 | MIT OR Apache-2.0 |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 |
| thiserror-impl | 2.0.12 | MIT OR Apache-2.0 |
| thread_local | 1.1.8 | MIT OR Apache-2.0 |
| time | 0.3.41 | MIT OR Apache-2.0 |
| time-core | 0.1.4 | MIT OR Apache-2.0 |
| time-macros | 0.2.22 | MIT OR Apache-2.0 |
| tinystr | 0.7.6 | Unicode-3.0 |
| tinyvec | 1.13.3 | Zlib OR Apache-2.0 OR MIT |
| tokio | 1.44.2 | MIT |
| tokio-native-tls | 0.3.1 | MIT |
| tokio-rustls | 0.26.2 | MIT OR Apache-2.0 |
| tokio-util | 0.7.15 | MIT |
| toml | 0.8.22 | MIT OR Apache-2.0 |
| toml_datetime | 0.6.9 | MIT OR Apache-2.0 |
| toml_edit | 0.19.15 | MIT OR Apache-2.0 |
| toml_edit | 0.20.7 | MIT OR Apache-2.0 |
| toml_edit | 0.22.26 | MIT OR Apache-2.0 |
| toml_write | 0.1.1 | MIT OR Apache-2.0 |
| tower | 0.5.2 | MIT |
| tower-layer | 0.3.3 | MIT |
| tower-service | 0.3.3 | MIT |
| tracing | 0.1.41 | MIT |
| tracing-appender | 0.2.3 | MIT |
| tracing-attributes | 0.1.28 | MIT |
| tracing-core | 0.1.33 | MIT |
| tracing-log | 0.2.0 | MIT |
| tracing-subscriber | 0.3.19 | MIT |
| tray-icon | 0.20.1 | MIT OR Apache-2.0 |
| try-lock | 0.2.5 | MIT |
| ts-rs | 12.0.1 | MIT |
| ts-rs-macros | 12.0.1 | MIT |
| typeid | 1.0.3 | MIT OR Apache-2.0 |
| typenum | 1.18.0 | MIT OR Apache-2.0 |
| uds_windows | 1.1.0 | MIT |
| unic-char-property | 0.9.0 | MIT/Apache-2.0 |
| unic-char-range | 0.9.0 | MIT/Apache-2.0 |
| unic-common | 0.9.0 | MIT/Apache-2.0 |
| unic-ucd-ident | 0.9.0 | MIT/Apache-2.0 |
| unic-ucd-version | 0.9.0 | MIT/Apache-2.0 |
| unicode-ident | 1.0.18 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| unicode-segmentation | 1.12.0 | MIT OR Apache-2.0 |
| untrusted | 0.9.0 | ISC |
| url | 2.5.4 | MIT OR Apache-2.0 |
| urlpattern | 0.3.0 | MIT |
| utf-8 | 0.7.6 | MIT OR Apache-2.0 |
| utf16_iter | 1.0.5 | Apache-2.0 OR MIT |
| utf8_iter | 1.0.4 | Apache-2.0 OR MIT |
| uuid | 1.16.0 | Apache-2.0 OR MIT |
| valuable | 0.1.1 | MIT |
| walkdir | 2.5.0 | Unlicense/MIT |
| want | 0.3.1 | MIT |
| wasi | 0.11.0+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| wasi | 0.14.2+wasi-0.2.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| wasi | 0.9.0+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| wasm-bindgen | 0.2.100 | MIT OR Apache-2.0 |
| wasm-bindgen-backend | 0.2.100 | MIT OR Apache-2.0 |
| wasm-bindgen-futures | 0.4.50 | MIT OR Apache-2.0 |
| wasm-bindgen-macro | 0.2.100 | MIT OR Apache-2.0 |
| wasm-bindgen-macro-support | 0.2.100 | MIT OR Apache-2.0 |
| wasm-bindgen-shared | 0.2.100 | MIT OR Apache-2.0 |
| wasm-streams | 0.4.2 | MIT OR Apache-2.0 |
| wayland-backend | 0.3.10 | MIT |
| wayland-client | 0.31.10 | MIT |
| wayland-protocols | 0.31.2 | MIT |
| wayland-protocols | 0.32.8 | MIT |
| wayland-protocols-wlr | 0.2.0 | MIT |
| wayland-scanner | 0.31.6 | MIT |
| wayland-sys | 0.31.6 | MIT |
| web-sys | 0.3.77 | MIT OR Apache-2.0 |
| web-time | 1.1.0 | MIT OR Apache-2.0 |
| webkit2gtk | 2.0.1 | MIT |
| webkit2gtk-sys | 2.0.1 | MIT |
| webpki-roots | 0.26.10 | CDLA-Permissive-2.0 |
| webpki-roots | 1.0.10 | CDLA-Permissive-2.0 |
| webview2-com | 0.37.0 | MIT |
| webview2-com-macros | 0.8.0 | MIT |
| webview2-com-sys | 0.37.0 | MIT |
| widestring | 1.2.0 | MIT OR Apache-2.0 |
| winapi | 0.3.9 | MIT/Apache-2.0 |
| winapi-i686-pc-windows-gnu | 0.4.0 | MIT/Apache-2.0 |
| winapi-util | 0.1.9 | Unlicense OR MIT |
| winapi-x86_64-pc-windows-gnu | 0.4.0 | MIT/Apache-2.0 |
| window-vibrancy | 0.6.0 | Apache-2.0 OR MIT |
| windows | 0.60.0 | MIT OR Apache-2.0 |
| windows | 0.61.1 | MIT OR Apache-2.0 |
| windows-collections | 0.1.1 | MIT OR Apache-2.0 |
| windows-collections | 0.2.0 | MIT OR Apache-2.0 |
| windows-core | 0.60.1 | MIT OR Apache-2.0 |
| windows-core | 0.61.0 | MIT OR Apache-2.0 |
| windows-future | 0.1.1 | MIT OR Apache-2.0 |
| windows-future | 0.2.0 | MIT OR Apache-2.0 |
| windows-implement | 0.59.0 | MIT OR Apache-2.0 |
| windows-implement | 0.60.0 | MIT OR Apache-2.0 |
| windows-interface | 0.59.1 | MIT OR Apache-2.0 |
| windows-link | 0.1.1 | MIT OR Apache-2.0 |
| windows-link | 0.2.1 | MIT OR Apache-2.0 |
| windows-numerics | 0.1.1 | MIT OR Apache-2.0 |
| windows-numerics | 0.2.0 | MIT OR Apache-2.0 |
| windows-registry | 0.4.0 | MIT OR Apache-2.0 |
| windows-result | 0.3.2 | MIT OR Apache-2.0 |
| windows-strings | 0.3.1 | MIT OR Apache-2.0 |
| windows-strings | 0.4.0 | MIT OR Apache-2.0 |
| windows-sys | 0.45.0 | MIT OR Apache-2.0 |
| windows-sys | 0.52.0 | MIT OR Apache-2.0 |
| windows-sys | 0.59.0 | MIT OR Apache-2.0 |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 |
| windows-targets | 0.42.2 | MIT OR Apache-2.0 |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 |
| windows-targets | 0.53.0 | MIT OR Apache-2.0 |
| windows-version | 0.1.4 | MIT OR Apache-2.0 |
| windows_aarch64_gnullvm | 0.42.2 | MIT OR Apache-2.0 |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 |
| windows_aarch64_gnullvm | 0.53.0 | MIT OR Apache-2.0 |
| windows_aarch64_msvc | 0.42.2 | MIT OR Apache-2.0 |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 |
| windows_aarch64_msvc | 0.53.0 | MIT OR Apache-2.0 |
| windows_i686_gnu | 0.42.2 | MIT OR Apache-2.0 |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 |
| windows_i686_gnu | 0.53.0 | MIT OR Apache-2.0 |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 |
| windows_i686_gnullvm | 0.53.0 | MIT OR Apache-2.0 |
| windows_i686_msvc | 0.42.2 | MIT OR Apache-2.0 |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 |
| windows_i686_msvc | 0.53.0 | MIT OR Apache-2.0 |
| windows_x86_64_gnu | 0.42.2 | MIT OR Apache-2.0 |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_gnu | 0.53.0 | MIT OR Apache-2.0 |
| windows_x86_64_gnullvm | 0.42.2 | MIT OR Apache-2.0 |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_gnullvm | 0.53.0 | MIT OR Apache-2.0 |
| windows_x86_64_msvc | 0.42.2 | MIT OR Apache-2.0 |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_msvc | 0.53.0 | MIT OR Apache-2.0 |
| winnow | 0.5.40 | MIT |
| winnow | 0.7.9 | MIT |
| wit-bindgen-rt | 0.39.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| write16 | 1.0.0 | Apache-2.0 OR MIT |
| writeable | 0.5.5 | Unicode-3.0 |
| wry | 0.51.2 | Apache-2.0 OR MIT |
| x11 | 2.21.0 | MIT |
| x11-dl | 2.21.0 | MIT |
| xcap | 0.5.1 | Apache-2.0 |
| xcb | 1.5.0 | MIT |
| yoke | 0.7.5 | Unicode-3.0 |
| yoke-derive | 0.7.5 | Unicode-3.0 |
| zbus | 5.6.0 | MIT |
| zbus_macros | 5.6.0 | MIT |
| zbus_names | 4.2.0 | MIT |
| zerocopy | 0.8.25 | BSD-2-Clause OR Apache-2.0 OR MIT |
| zerocopy-derive | 0.8.25 | BSD-2-Clause OR Apache-2.0 OR MIT |
| zerofrom | 0.1.6 | Unicode-3.0 |
| zerofrom-derive | 0.1.6 | Unicode-3.0 |
| zeroize | 1.8.1 | Apache-2.0 OR MIT |
| zerovec | 0.10.4 | Unicode-3.0 |
| zerovec-derive | 0.10.3 | Unicode-3.0 |
| zvariant | 5.5.1 | MIT |
| zvariant_derive | 5.5.1 | MIT |
| zvariant_utils | 3.2.0 | MIT |

## 界面依赖的 npm 包（28 个）

| 名称 | 版本 | 许可 |
| --- | --- | --- |
| @babel/helper-string-parser | 7.25.9 | MIT |
| @babel/helper-validator-identifier | 7.25.9 | MIT |
| @babel/parser | 7.27.0 | MIT |
| @babel/types | 7.27.0 | MIT |
| @jridgewell/sourcemap-codec | 1.5.0 | MIT |
| @tauri-apps/api | 2.12.4 | Apache-2.0 OR MIT |
| @tauri-apps/api | 2.4.1 | Apache-2.0 OR MIT |
| @tauri-apps/plugin-dialog | 2.8.3 | MIT OR Apache-2.0 |
| @tauri-apps/plugin-opener | 2.2.6 | MIT OR Apache-2.0 |
| @vue/compiler-core | 3.5.13 | MIT |
| @vue/compiler-dom | 3.5.13 | MIT |
| @vue/compiler-sfc | 3.5.13 | MIT |
| @vue/compiler-ssr | 3.5.13 | MIT |
| @vue/reactivity | 3.5.13 | MIT |
| @vue/runtime-core | 3.5.13 | MIT |
| @vue/runtime-dom | 3.5.13 | MIT |
| @vue/server-renderer | 3.5.13 | MIT |
| @vue/shared | 3.5.13 | MIT |
| csstype | 3.1.3 | MIT |
| entities | 4.5.0 | BSD-2-Clause |
| estree-walker | 2.0.2 | MIT |
| magic-string | 0.30.17 | MIT |
| nanoid | 3.3.11 | MIT |
| picocolors | 1.1.1 | ISC |
| postcss | 8.5.3 | MIT |
| source-map-js | 1.2.1 | BSD-3-Clause |
| typescript | 5.6.3 | Apache-2.0 |
| vue | 3.5.13 | MIT |
