# Changelog

All notable changes to tein are documented here.
Versions follow [Semantic Versioning](https://semver.org/).

## [0.2.3] - 2026-03-11

### Bug Fixes

- Workaround chibi GC corruption during module import ([`b34e881`](https://github.com/emesal/tein/commit/b34e8811b1f1ba365398bc95ef4af6845da4ae6d))

- Chunk VFS string literals to avoid MSVC C2026 limit; ignore /vendor/ ([`afb6a47`](https://github.com/emesal/tein/commit/afb6a47fdee0db20ea8e8a169bab797c822adb4f))

- **serde:** Accept string keys in alist heuristic ([`e7e5d1a`](https://github.com/emesal/tein/commit/e7e5d1af21c3b98a88e5b2413c68b54dc710d2e8))

- **serde:** Error on u64 overflow instead of silent f64 truncation ([`3396ef5`](https://github.com/emesal/tein/commit/3396ef54008a15b75d68fe2ee736b8ca63129674))

- **serde:** Explicit error messages for i128/u128 ([`c388471`](https://github.com/emesal/tein/commit/c388471baab9b79a52a280e21ed57107998ae10f))

- Reader_unset_wrapper UB, tein_make_error dead allocation ([`4a623c0`](https://github.com/emesal/tein/commit/4a623c009d22e7d697b542f106aea8d897b249e7))

- All important code review issues ([`9278ce6`](https://github.com/emesal/tein/commit/9278ce639af2e4679259d749c6c8447921005ed5))

- All suggestion-level code review issues ([`643063e`](https://github.com/emesal/tein/commit/643063e335d0184e8ea1cfc486c7b5ef3c22c2cf))

- Always stub eval/interaction-environment and friends in sandboxed contexts ([`9565414`](https://github.com/emesal/tein/commit/95654145539db6c8214812ca909e0ce56a71954c))

- Add per-iteration counter in analyze() to cap macro hook re-analysis loop ([`6c0b6a0`](https://github.com/emesal/tein/commit/6c0b6a06d104bfca3b643fd413dd2d63eb25190a))

- Validate port trampoline start/end indices before pointer arithmetic ([`da36d24`](https://github.com/emesal/tein/commit/da36d24ad1175ad38df48bb29550ba8c38d284c9))

- Catch thread panics in managed/timeout contexts, return InitError instead of hanging ([`aab2f30`](https://github.com/emesal/tein/commit/aab2f304b6c05689947a1831054568aee4b59f92))

- Save/restore thread-local policy on context drop to prevent sequential context interference ([`2f8d252`](https://github.com/emesal/tein/commit/2f8d2529144cb73e71d3cdd6352830832e95437e))

- Use strict from_utf8 in extract_exception_error to prevent lossy UTF-8 corrupting sentinel detection ([`6f24097`](https://github.com/emesal/tein/commit/6f24097016743fd4fddb459bd141b183dcd36a68))

- Use xorshift64 PRNG for unpredictable port/foreign handle IDs ([`271d7f7`](https://github.com/emesal/tein/commit/271d7f76a070d5e658f1a27f6c7cd49b1e80aa8c))

- Add iteration limit to env_copy_named parent-chain walk (cycle detection) ([`1643f37`](https://github.com/emesal/tein/commit/1643f37a790d961028f6c7126d10ce36851ce3e6))

- Use CString consistently in sandbox error path (was bare string ptr cast) ([`c319731`](https://github.com/emesal/tein/commit/c319731953d2042c847e38a6110e3de6e8d9e838))

- Use bounded sync_channel(64) in ThreadLocalContext to prevent unbounded memory growth ([`ef65526`](https://github.com/emesal/tein/commit/ef65526cb0506d3a30b5e9b5ceb911a3024645e7))

- Add bounds assertion to tein_sexp_vector_set ([`fe6b339`](https://github.com/emesal/tein/commit/fe6b33904d518ec7e5920016c05ec3ed50751b38))

- Extend ALWAYS_STUB with additional dangerous chibi primitives from opcodes.c audit ([`a18342d`](https://github.com/emesal/tein/commit/a18342ddc178c0651970926e6a9b6bd321c52ae4))

- Eliminate all mem::transmute fn-pointer casts via sexp_proc2 shim ([`13229d6`](https://github.com/emesal/tein/commit/13229d6eed66b3f2717099ffcc22374df8540a25))

- Root all critical GC-invisible sexp values across allocation points ([`5cc4e69`](https://github.com/emesal/tein/commit/5cc4e691d9f331e7997bd82f8661d69b06587d43))

- Resolve all remaining c code review findings (H1, M1-M4, L1-L6) ([`ddf0ab2`](https://github.com/emesal/tein/commit/ddf0ab27e46b58a3589f7bcf2d4071774821bfa0))

- Resolve H7-H8 in chibi fork, mitigate H9 with defensive comment ([`1634958`](https://github.com/emesal/tein/commit/1634958a16d3a819d1d80f34fc27d2042f2b1c6f))

- Resolve H12 bignum div-by-zero in fork, mark H13 already resolved ([`32531a6`](https://github.com/emesal/tein/commit/32531a6b0303367c34e9c83c1603491e734f7f48))

- Serialise ThreadLocalContext send+recv as atomic roundtrip ([`37efeda`](https://github.com/emesal/tein/commit/37efeda5253fdf1b360f8fc7253e11e099563bea))

- Clippy clean — collapse if-let, allow forward-looking doc fields (#60) ([`8316d6c`](https://github.com/emesal/tein/commit/8316d6c04404f852a1229c272f3fd5475ebbeb3e))

- Suppress false "importing undefined variable" warnings (#57) ([`bbab40a`](https://github.com/emesal/tein/commit/bbab40a06dc59a6d2618ed1916aceb6195178d6f))

- Use already-prefixed names for ext foreign type convenience procs (#69) ([`78454da`](https://github.com/emesal/tein/commit/78454da6e6539d78f54ce57345c92d7566c660b6))

- Gc safety + error handling bugs in numeric tower ([`f788066`](https://github.com/emesal/tein/commit/f788066470ed9e4c416b26d6c3fc51a2e3a299bd))

- Alist Serialize emits maps, not arrays ([`f23f9c8`](https://github.com/emesal/tein/commit/f23f9c8e3277d9db6c86d325fccb7bbcbcde1104))

- **json:** '() stringifies to [] not null; document empty {}/{} ambiguity ([`c58a7af`](https://github.com/emesal/tein/commit/c58a7afa3be28febc5a62fe49cc22965fa9929c8))

- **gc:** Root eval result before from_raw conversion ([`506a386`](https://github.com/emesal/tein/commit/506a386e0ab9bb26a5515c03f28b8d43dfda255c))

- **gc:** Root expr between sexp_read and sexp_evaluate, closes #76 ([`1a2a6ea`](https://github.com/emesal/tein/commit/1a2a6ea9434d265e218ac4e0d73ced9b06f9acd9))

- **json:** Correct Nil→[] and null-symbol→null in serde paths ([`59e5331`](https://github.com/emesal/tein/commit/59e5331d25b89d418e32c08577a72fe508fc87fd))

- **toml:** Cast sexp_sint_t to i64 for windows (c_long is i32 on LLP64) ([`9cc7c34`](https://github.com/emesal/tein/commit/9cc7c3424c52a20f39e050e0edf63563ad48987e))

- Add scheme/base + scheme/write to IMPLICIT_DEPS; gate only .sld files in allowlist (#86) ([`23f786c`](https://github.com/emesal/tein/commit/23f786ce2b9c271e3b02c332cc7dde101887ac7f))

- GC root safety, extract trampoline helpers, sync load docs (#87) ([`4e4261c`](https://github.com/emesal/tein/commit/4e4261cc936c36cb2633d7dc3c613f8bce104f5c))

- Value arg error propagation + sandbox test + docstring gaps ([`2559c0d`](https://github.com/emesal/tein/commit/2559c0df113d925e67e8528fae40e3c4b6adeee2))

- Correct stale test comment in test_vfs_gate_all ([`d9b993c`](https://github.com/emesal/tein/commit/d9b993cfbcc93f8f08cbaac54553328478654025))

- Exclude posix-only srfi/18 clib on windows to fix msvc build ([`231f6b1`](https://github.com/emesal/tein/commit/231f6b109106396a7c7c2c1c7cdc1e0504822f4d))

- **sandbox:** Inject srfi/98 shadow to neuter get-environment-variable ([`eabb23f`](https://github.com/emesal/tein/commit/eabb23fdeaa30bf6ebf134b3f2e08e54c6e9bc3f))

- **ffi:** GC root sexp_c_str result in sandboxed command_line_trampoline ([`376e07a`](https://github.com/emesal/tein/commit/376e07aa602d9b673f815f0960f72fcc9dd594a8))

- Correct stale Modules::Safe docstring — repl/process/file are in Safe via shadows ([`52904e5`](https://github.com/emesal/tein/commit/52904e5e0f035173e9283deb31ab857f427cb10d))

- Add missing ClibEntry for chibi/weak and scheme/time (closes #98 partial) ([`96c65e0`](https://github.com/emesal/tein/commit/96c65e0665c33aafbcc26135af77c8e0e8a3cfce))

- Dedup unexported_stubs + add scheme/small and scheme/red to VFS registry ([`61ea8fc`](https://github.com/emesal/tein/commit/61ea8fceea21587d7e0ec88dead7bbbd97e1a510))

- Allow dead_code on SHADOW_STUBS (used by build.rs via include!) ([`72ed284`](https://github.com/emesal/tein/commit/72ed284933e4c51772ba88c2be5915afd3711684))

- Skip generated shadow stubs in extract_exports (shadow_sld: None) ([`faa1652`](https://github.com/emesal/tein/commit/faa16521ed6fee2baa9f9d246ddb1fa32ac41e62))

- Update 3 tests — chibi/process and chibi/net now in VFS, use chibi/app for blocked-import assertions ([`d1c7ae4`](https://github.com/emesal/tein/commit/d1c7ae4f0d84aa54442f4219ef91a02e8f2a8fde))

- Dedup vfs_files before generate_vfs_data — removes redundant binary data ([`a9c4b22`](https://github.com/emesal/tein/commit/a9c4b224eebeeab06419304087309de6c1c403cf))

- Add ClibEntry for srfi/144, scheme/bytevector, chibi/time ([`c06eb73`](https://github.com/emesal/tein/commit/c06eb73fbdafbf3a321dfd8c240a06b572cb66f8))

- Correct srfi/144 clib vfs_key to include /math stem ([`dcf97fd`](https://github.com/emesal/tein/commit/dcf97fd663138fd8c4718d44e3582044fb387b16))

- Validate_include_shared — build-time check for include-shared without ClibEntry ([`3d700ea`](https://github.com/emesal/tein/commit/3d700ea731dfc36db33cd502db2dc8ea38d75962))

- Update test_chibi_channel_in_vfs — chibi/time clib enables full srfi/18 on posix ([`6f26c10`](https://github.com/emesal/tein/commit/6f26c10491b32b314d28fea43f4ed1078c8c8e58))

- Scheme/flonum test uses r7rs fl= fl< fl> names (chibi fork updated) ([`e328b81`](https://github.com/emesal/tein/commit/e328b81b53881e0e2015e8c69c9007f62ac4243a))

- Set (chibi regexp) / srfi-115 / scheme/regex to default_safe: false ([`10674f0`](https://github.com/emesal/tein/commit/10674f03856c7b378493fd9118860439776b99aa))

- Address code review findings on srfi-19 branch ([`211e8b9`](https://github.com/emesal/tein/commit/211e8b9105710dede387c8f3b94e2ba6009aa82b))

- **macros:** #[tein_fn] Value return type + compile_error on unsupported types ([`9e8469e`](https://github.com/emesal/tein/commit/9e8469e90725bcc24c9c60017ca13542af930983))

- **safe-regexp:** Skip scheme helper evaluation in sandboxed contexts (#116) ([`050dace`](https://github.com/emesal/tein/commit/050daceeba496095b6a4a0af675af8dd00857b6e))

- **time:** Gate localtime_r behind #[cfg(unix)], add windows + fallback impls ([`df35dd2`](https://github.com/emesal/tein/commit/df35dd282509ebe85c718d5554db76fd8de67c4a))

- **ffi:** Guard sexp_car against null args in variadic trampolines ([`42e9d49`](https://github.com/emesal/tein/commit/42e9d498ada1eb642b244b7ec7ab0b0aa2c263d4))

- **repl:** Flush stdout after each eval to fix buffered display output ([`ee69475`](https://github.com/emesal/tein/commit/ee694751dfa05dcaec2f49ba0b705d8ec026a550))

- **repl:** Flush chibi output port before returning to rustyline ([`98309ec`](https://github.com/emesal/tein/commit/98309ec315e81f99cf3f92498414db05d888a86d))

- **repl:** Eliminate blank lines + enable streaming flush ([`8c87949`](https://github.com/emesal/tein/commit/8c87949dc95eca06887f9d30c13f30d17d3b1203))

- **repl:** Reset tracker between evals to prevent stale newline state ([`f0ec760`](https://github.com/emesal/tein/commit/f0ec7601232381218d256aaeab6040b40e0169fb))

- **ffi:** Propagate OOM from tein_vfs_register as Error instead of abort ([`f61d0ca`](https://github.com/emesal/tein/commit/f61d0ca367da173e4b64727eac79aa1675124d2f))

- **ffi:** GC rooting in environment_trampoline cons loop (#97) ([`019fcb0`](https://github.com/emesal/tein/commit/019fcb070734e21d1c02b63238a1a5220cc96678))

- **tests:** Fix stale comment and strengthen content-type assertion (#135) ([`a26231e`](https://github.com/emesal/tein/commit/a26231ed4720c2288b2ce64c34500e1682b1e343))

- **context:** Gc-root c_dir before add_module_directory; support -I./lib; fix AGENTS.md invariant ([`d1c89c9`](https://github.com/emesal/tein/commit/d1c89c9ae49fc37d09b1b6a0db8ed33c6f1f6bb4))

- **context:** Rename exit trampoline registration to emergency-exit (#101) ([`6ccc026`](https://github.com/emesal/tein/commit/6ccc026a036a3cee4f74f24c5a10d6f141ff7b34))

- **context,vfs:** Wire emergency-exit for (tein process) scheme exit (#101) ([`b8460da`](https://github.com/emesal/tein/commit/b8460dac2d3c12a8d34d61fc029e4bafe4c5eb56))

- R7rs-compliant exit with dynamic-wind cleanup ([`d282319`](https://github.com/emesal/tein/commit/d2823192681b60bf1603155968e34af640cbc07a))

- **process:** Flush-not-close on exit; document double-registration ([`b272cd4`](https://github.com/emesal/tein/commit/b272cd49628dbfe9aa0e0e5f6d0c6eec3807449f))

- **vfs:** Remove "chibi" pseudo-dep from tein/process registry entry ([`2cf2477`](https://github.com/emesal/tein/commit/2cf2477328ebd980825ae7e14dffa635d7cb088f))

- **context:** Register process trampolines into primitive env to override chibi builtins ([`656a7a4`](https://github.com/emesal/tein/commit/656a7a44bffaba186e94293f53b91aac354a3fde))

- Make test_chibi_diff robust against terminal environment ([`27f1ce7`](https://github.com/emesal/tein/commit/27f1ce7def2fdbe272d4ffae7b90208d9f6a68e6))

- Make test_chibi_diff robust against terminal environment ([`1680f94`](https://github.com/emesal/tein/commit/1680f943d1b67441806fa469f7251c07ac114a2d))


### Chores

- Add .worktrees/ to gitignore ([`76d2984`](https://github.com/emesal/tein/commit/76d29849bcf763c1952ec28ae614ab8b005220c9))

- Update implementation plan progress to tasks 1-6 complete (#62) ([`0df82d1`](https://github.com/emesal/tein/commit/0df82d1557abeb9364cadc0f3850d83ac61c1010))

- Standardise ISC licence and publish = false across all crates (#67) ([`3c697e4`](https://github.com/emesal/tein/commit/3c697e47308348cb98a4fd0579154274830f1e6d))

- Apply rustfmt formatting ([`776c36d`](https://github.com/emesal/tein/commit/776c36de7be4db76aff5525a8d1095031606836c))

- Add extern crate self alias + uuid dependency ([`63061b5`](https://github.com/emesal/tein/commit/63061b57a4edaffb591b84a558b07f8b474e4c04))

- Remove superseded uuid plan (replaced by 2026-03-01) ([`5e31c66`](https://github.com/emesal/tein/commit/5e31c66da9e02e92c7081a91141c6179f092753c))

- Final cleanup — AGENTS.md patterns + mark plan complete ([`83d071a`](https://github.com/emesal/tein/commit/83d071a591876637859e1f9942b74d1743b8a32f))

- Mark VfsGate refactor plan complete (steps 6–8) ([`7948dac`](https://github.com/emesal/tein/commit/7948dac2d2c7aafe9133ce7e988280382f18917b))

- Lint fixes, remove old constants, update plan (batch 1 checkpoint) ([`94b09f2`](https://github.com/emesal/tein/commit/94b09f2ab9b65b4cd17b497e6529af0938845029))

- Update plan with batch 2 progress notes ([`1108f01`](https://github.com/emesal/tein/commit/1108f016ca44b9e0a56aa13aacb43eeea5f4d84f))

- Lint fixes, update plan (batch 3 checkpoint) ([`ce0d211`](https://github.com/emesal/tein/commit/ce0d211cf14af2f796ec53123fe1de5bf2c6a246))

- Update plan with batch 4 completion notes ([`238a2dc`](https://github.com/emesal/tein/commit/238a2dc29bc577c3991d1902dba3147cfd64d288))

- Scaffold tein-bin crate ([`a4eb2c7`](https://github.com/emesal/tein/commit/a4eb2c704cbe0ffd3653a8073a99af449366c9f0))

- **crypto:** Lint + docs for (tein crypto) (#38), closes #38 ([`6b10986`](https://github.com/emesal/tein/commit/6b10986dd298dc17cd5f87df3208eac84af9fdfd))

- Apply cargo fmt (#132) ([`0a881e4`](https://github.com/emesal/tein/commit/0a881e492fa562c8e21b7cf2040875cc3c36d984))

- Cargo fmt + clippy fix (#130) ([`2110776`](https://github.com/emesal/tein/commit/21107765bf6d74100a11cf424d6302ca0dca30e4))

- Update Cargo.lock for tempfile dev-dep (#131) ([`21a2377`](https://github.com/emesal/tein/commit/21a2377aec4e7de6c4625a40845c1cff939b2bbf))

- Fix clippy collapsible-if in auto-import block ([`8455213`](https://github.com/emesal/tein/commit/845521313a74d6d419a556d3dc29429cbd0d0160))

- Add git-cliff changelog generation to release workflow ([`bec8629`](https://github.com/emesal/tein/commit/bec8629840ffedfcb0b8a5574c860b6f5d0b752f))


### Documentation

- Migrate handoff.md into DEVELOPMENT.md and AGENTS.md, remove handoff ([`018d399`](https://github.com/emesal/tein/commit/018d399ee90dab24e474eac7048ec11e3a0628cb))

- Update known limitations — hash table/port/continuation notes ([`8991491`](https://github.com/emesal/tein/commit/89914916f05c63238e4a2138ce3bcf903260cfd4))

- Mark REPL example complete in TODO ([`4e8e67a`](https://github.com/emesal/tein/commit/4e8e67a25984b45cafe0934e83af819dd4ad1c97))

- **serde:** Add doc-tests to all public serde API functions ([`bb1d3ad`](https://github.com/emesal/tein/commit/bb1d3adfa52b092b1a1a0f9d444f0dcbdf3bb9c5))

- Mark serde data format complete in TODO ([`b13e040`](https://github.com/emesal/tein/commit/b13e040d9a98dc3366f587fc0c613c57796ef075))

- Foreign type protocol example, architecture docs, roadmap update ([`84f7b2a`](https://github.com/emesal/tein/commit/84f7b2ae694e09a358724c3d16c6152b2eed9235))

- Add ThreadLocalContext to architecture docs and roadmap ([`d517be3`](https://github.com/emesal/tein/commit/d517be3772c60f9097cc376c6de5270968ec66ad))

- Add ThreadLocalContext implementation plan ([`37d92a1`](https://github.com/emesal/tein/commit/37d92a19ad368406a5850899c49c63051c5d26b4))

- Update macro expansion hooks plan with progress and deviations ([`00b5103`](https://github.com/emesal/tein/commit/00b5103e30c1bcc756a14314e55c68a0836ceb8d))

- Update architecture and roadmap for macro expansion hooks ([`99bdbf0`](https://github.com/emesal/tein/commit/99bdbf05cf6ff6e9ee0a610ef7b92507bf180c1e))

- Rewrite README with substance-first structure ([`fdf0fb5`](https://github.com/emesal/tein/commit/fdf0fb5df98416f795a2e3ee35774b01baaf95f6))

- Fix 7 rustdoc link resolution warnings ([`73d80b1`](https://github.com/emesal/tein/commit/73d80b12b2df3844d8f13c0a18a9faeceab5bf8b))

- Expand crate-level rustdoc with feature overview and safety model ([`9595b53`](https://github.com/emesal/tein/commit/9595b534a25d666181daffb3a970e12780e92c49))

- Context module rustdoc — builder, eval, sandboxing overview ([`fa99663`](https://github.com/emesal/tein/commit/fa99663ac14848b8123a877cbac73562a11b8629))

- Sandbox module — presets reference, security model, composition guide ([`5d0fddf`](https://github.com/emesal/tein/commit/5d0fddf05b61cad42153d5646350eb54cacfdc5a))

- Foreign module — dispatch chain, complete working example ([`74e2a2d`](https://github.com/emesal/tein/commit/74e2a2dbcc630817a3c17f5eac1dc9539234cd43))

- Managed, timeout, value, error module rustdoc ([`b73a4af`](https://github.com/emesal/tein/commit/b73a4af54fa80ba33e4eeb2f18f7e03ac550a232))

- Rename DEVELOPMENT.md to ARCHITECTURE.md, update stale content ([`86b002e`](https://github.com/emesal/tein/commit/86b002e002504286256b47ae8fc83ffdb9e5235e))

- Sentence case throughout, add newcomer guide ([`7c82f21`](https://github.com/emesal/tein/commit/7c82f214ed44221f052fa9eb7690bff84e6690e6))

- Record code review findings for bugfix/mvp-code-review-2602 ([`0881d30`](https://github.com/emesal/tein/commit/0881d30713056fa7f32bc0d94d14413ad7ee389e))

- Mark critical issues resolved in review plan ([`4ea402d`](https://github.com/emesal/tein/commit/4ea402d713493a4200d4d25035a50cca41cbc951))

- Full codebase security audit findings ([`8aa962c`](https://github.com/emesal/tein/commit/8aa962c851aa3c7d2f0273fdda54469c33ce9c1e))

- Mark tasks 1-3 complete, add implementation notes to security audit plan ([`3e2ec08`](https://github.com/emesal/tein/commit/3e2ec08baf848978845afd54d230755a2fa85700))

- Mark tasks 4-6 complete, update progress in security audit plan ([`b501e19`](https://github.com/emesal/tein/commit/b501e19da1c886debbb815fefb9f5fd3d29683db))

- Issue #11 (u64 ID overflow) resolved by xorshift64 fix in Task 7 ([`3bbb0f3`](https://github.com/emesal/tein/commit/3bbb0f3f7e868f3aac9c6f293302cf89cad0aed1))

- Document ASCII-only limitation of reader dispatch table ([`7e8f5ec`](https://github.com/emesal/tein/commit/7e8f5ecb96046620ea743e6004c1db5cab6efee6))

- Mark all security audit issues resolved, add resolution status table ([`5a58ec3`](https://github.com/emesal/tein/commit/5a58ec397f44e2c92c5efa08adc4995a791c08d1))

- Append issue #13 implementation plan (fn pointer transmute fix) to audit doc ([`ebbbb52`](https://github.com/emesal/tein/commit/ebbbb525a9c511fa37e8bdcb04a0435c5a235829))

- C code review findings — GC roots, macro hook safety, defence in depth ([`7bd842a`](https://github.com/emesal/tein/commit/7bd842a835a2ff2d87e3073fca16754c07162a81))

- Mark critical C1-C3 resolved in c code review plan ([`c0946c1`](https://github.com/emesal/tein/commit/c0946c18e64c862d7c9ee69aed5244050f143bb7))

- Downgrade H1-H3 GC finaliser bugs to mitigated, add defensive comments ([`82e70d3`](https://github.com/emesal/tein/commit/82e70d3c2cdcdbd4539475aeab6fa4d3c71a36ee))

- Downgrade H4-H6 GC/heap overflow bugs to mitigated ([`3fc6cea`](https://github.com/emesal/tein/commit/3fc6ceaa3d92827fa16c5f9c91fbfa6b04edd73c))

- Update H7/H8 resolved commit hashes in review plan ([`73de5cb`](https://github.com/emesal/tein/commit/73de5cb406a2e4f68f821e58b7640a44d033080f))

- Fix summary table — count M15 (fixed on master) as resolved ([`53b6a26`](https://github.com/emesal/tein/commit/53b6a26ce67c79fe8b32c3a3fbad9e0b13dddf01))

- Resolve M1-M3 reader safety issues, add methodology to plan ([`50889f4`](https://github.com/emesal/tein/commit/50889f4176f2f54fed70762c1d7ce494f1f9ebb8))

- Resolve M4-M8 evaluator safety issues in review plan ([`d6b78cd`](https://github.com/emesal/tein/commit/d6b78cdf057c20f959f6ae82f4a14ae1b971a58a))

- Resolve M9-M17, correct M18, add safety invariants checklist ([`b271702`](https://github.com/emesal/tein/commit/b271702fd3ada3dd1f1c36c1a7982dc09c2c21ec))

- Replace TODO.md with ROADMAP.md ([`8680637`](https://github.com/emesal/tein/commit/868063798e121682e115bff811d1324b033aad16))

- Triage all lows, rebase chibi fork onto master, fix L7/L11/L12/L15 ([`d3cce1d`](https://github.com/emesal/tein/commit/d3cce1d4e78623ece32cb1ecfd3aa2fb7d578af0))

- Update scheme test coverage plan with progress + chibi quirks ([`6167df4`](https://github.com/emesal/tein/commit/6167df437e5cb3466c83c6dc1c8976fa9d1c8aeb))

- #[tein_module] proc macro design (issue #40) ([`24f9ffd`](https://github.com/emesal/tein/commit/24f9ffddfb37dbf172aea37b7a80d6d8f8f80825))

- #[tein_module] implementation plan (12 tasks) ([`3b698fd`](https://github.com/emesal/tein/commit/3b698fd63f1a05294f2d72c0851e1a2a35a9f5f4))

- Update tein-module plan with progress (tasks 1-3 complete) ([`63cb7c4`](https://github.com/emesal/tein/commit/63cb7c42242f3edd7955b77394795a7591c9fcab))

- Add worktree path to plan for session resume ([`a35a09d`](https://github.com/emesal/tein/commit/a35a09d09fd8c14117263e60b9cd04ac8adb3806))

- Update tein-module plan — tasks 4-8 codegen complete, note resumption point ([`381f34b`](https://github.com/emesal/tein/commit/381f34b4ffc28a619ea4817071ffdebcb0cd2557))

- Design for doc attr scraping in #[tein_module] (#60) ([`52fccb4`](https://github.com/emesal/tein/commit/52fccb4474466b4e02ead0c287d36396fef3753a))

- Implementation plan for doc attr scraping (#60) ([`ee74051`](https://github.com/emesal/tein/commit/ee7405104299d575b054befcf14f533a81420c79))

- Update test count after #60 ([`0c4f7fc`](https://github.com/emesal/tein/commit/0c4f7fc817b43b800f6373e2f34c71589225b570))

- Design + implementation plan for (tein docs) module (#61) ([`338f773`](https://github.com/emesal/tein/commit/338f7733d2b1f74a6b5bee8c8dfbae2a75e45c64))

- Design for cdylib extension system (#62) ([`a5aa936`](https://github.com/emesal/tein/commit/a5aa936242c4155bd44deac4ce025eae44d98851))

- Implementation plan for cdylib extension system (#62) ([`99a0230`](https://github.com/emesal/tein/commit/99a023060a7a7928ae78becd5be886325b3d15b3))

- Update architecture for cdylib extension system (#62) ([`5689223`](https://github.com/emesal/tein/commit/5689223149987de6e6a5f00a35c0b35d359059a8))

- Add code comments for tein module quirks (#62) ([`b063130`](https://github.com/emesal/tein/commit/b063130ffb81705bbeeff8df636997c4947214ef))

- (tein json) design — built-in JSON module via Value ↔ Sexp bridge (#36) ([`fdb7034`](https://github.com/emesal/tein/commit/fdb7034acce8f045311c34442e5687b8be79ce23))

- Type parity design — numeric tower + bytevector for Value ↔ Sexp bridge (#71) ([`e59c83a`](https://github.com/emesal/tein/commit/e59c83abd30462528afe9915cdcad6bd3ca6299c))

- Type parity implementation plan — 10-task bottom-up numeric tower (#71) ([`ddbea57`](https://github.com/emesal/tein/commit/ddbea57e19483f47dd446bc616c7d6ba40f31dfe))

- Mark tasks 1-5 done in type parity plan (#71) ([`bbf8df5`](https://github.com/emesal/tein/commit/bbf8df5264228417940ba9e4d783c66f3a8304e9))

- Mark tasks 6-8 done in type parity plan (#71) ([`6187d3c`](https://github.com/emesal/tein/commit/6187d3cf414fd68d3c1ef9570385e0cb7e34a446))

- Update AGENTS.md with numeric tower variants and type check ordering (#71) ([`c1ddf9b`](https://github.com/emesal/tein/commit/c1ddf9bc8ea10faabd0f6e243f0086ccd9fa3e05))

- Mark tasks 9-10 done in type parity plan (#71) ([`3e85e1a`](https://github.com/emesal/tein/commit/3e85e1aaccd59f6d3e8833e15efa39c96b0ccc97))

- Add (tein json) implementation plan ([`a7ece6c`](https://github.com/emesal/tein/commit/a7ece6c2f34a6018dcd3c4eade77695daae3bd7f))

- Update design doc status + AGENTS.md for (tein json) ([`9224ced`](https://github.com/emesal/tein/commit/9224cedfa920d01a5bf6e3ecb2ece6d9f957f34e))

- Plan for feature-gating format modules (#78) ([`c8ad378`](https://github.com/emesal/tein/commit/c8ad3785c5be4cb728590784d440fb756d516ca9))

- Design for (tein toml) module (#77) ([`cacf723`](https://github.com/emesal/tein/commit/cacf723983d63418039769762f3c032d2a1bf3f1))

- Implementation plan for (tein toml) (#77) ([`9f0f355`](https://github.com/emesal/tein/commit/9f0f35551cd8b165782d46761f4983db5fd1d812))

- Update lib.rs feature table and AGENTS.md for (tein toml) (#77) ([`83fe801`](https://github.com/emesal/tein/commit/83fe8016ad214c9510de2d37ed1f1c90e5db2285))

- Design for VfsSafe/VfsAll module policy tiers (#86) ([`b93006d`](https://github.com/emesal/tein/commit/b93006d8e57bbf714e7277614210210ed9a13729))

- Implementation plan for VfsSafe/VfsAll module policy tiers (#86) ([`b9d118e`](https://github.com/emesal/tein/commit/b9d118eeac330e78ee1acb9d90916a8236845d7a))

- Update implementation plan with progress and bootstrap context (#86) ([`3edd9f2`](https://github.com/emesal/tein/commit/3edd9f2560f35a4bfa9fb402bbcd4167a834dfd4))

- Update module policy docs for three-tier model (#86) ([`f1e179d`](https://github.com/emesal/tein/commit/f1e179dd1db723ab67c892b8cf6b88e3e594d037))

- Document excluded scheme modules in SAFE_MODULES (#86) ([`074f66b`](https://github.com/emesal/tein/commit/074f66bac1416f0371540a9c300e1497a4f77384))

- Design for (tein file), (tein load), (tein process) (#87) ([`84a4321`](https://github.com/emesal/tein/commit/84a432105c72b5509c3e8ebc9a89021aeb37d1c0))

- Implementation plan for (tein file/load/process) (#87) ([`20909c2`](https://github.com/emesal/tein/commit/20909c24ed685b12e6993c577bcee4ba3227754a))

- Update implementation plan to reflect tasks 1-5 progress (#87) ([`79f452b`](https://github.com/emesal/tein/commit/79f452bd07f0e41bfc29ce9f85a3f08e2a3a5208))

- Update AGENTS.md and sandbox docs for #87 modules ([`ff0b91d`](https://github.com/emesal/tein/commit/ff0b91d92909df21f2140c3a613ec5058a9e83cc))

- Update plan with task 6-10 progress and (tein load) blocker (#87) ([`97a8655`](https://github.com/emesal/tein/commit/97a86557e9f068ba9b030e8145818dd973dd9701))

- Update implementation plan to reflect task 7 fix + full test pass (#87) ([`3d81bf8`](https://github.com/emesal/tein/commit/3d81bf84287207a74a922c9b419b9023392c5ec1))

- (tein uuid) design for #39 ([`2b52e45`](https://github.com/emesal/tein/commit/2b52e45c30d598d0a1128fa049c10fb183f50fa9))

- (tein uuid) implementation plan ([`5269355`](https://github.com/emesal/tein/commit/526935531a04be9bcfa1c4681d2173184a5086b6))

- Update feature flags and AGENTS.md for (tein uuid) ([`9b1714b`](https://github.com/emesal/tein/commit/9b1714b8bae8040610d90750174e0b910dad5550))

- Update VfsGate refactor plan — steps 4–5 complete ([`a312703`](https://github.com/emesal/tein/commit/a312703063ff7db52df409d3312601618d44890f))

- Update all references from ModulePolicy to VfsGate (step 6) ([`a326ced`](https://github.com/emesal/tein/commit/a326ced3c9010f9d9026facaef70c5af2170998c))

- Design for (tein time) sandbox-safe time module (#90) ([`b8bb54c`](https://github.com/emesal/tein/commit/b8bb54c82649225a65388551902efc0514c3b950))

- Add implementation plan for (tein time) (#90) ([`8b52308`](https://github.com/emesal/tein/commit/8b52308fbecda7c502471d36ebed95cd14b21be1))

- Update AGENTS.md and sandbox docs for (tein time), closes #90 ([`462ca4e`](https://github.com/emesal/tein/commit/462ca4ea28e9f88d3911dbbf37f8f9b2bd547af5))

- Note JIFFY_EPOCH process-global behaviour in critical gotchas ([`cd98681`](https://github.com/emesal/tein/commit/cd9868166fb5bb6ca96b44aa06103bde8e6817ea))

- Trim AGENTS.md, extract niche chibi FFI reference ([`0d80152`](https://github.com/emesal/tein/commit/0d8015291bba615574536f8c02e61617d1d92a27))

- Trim flow paragraphs in AGENTS.md architecture section ([`53423e7`](https://github.com/emesal/tein/commit/53423e79576ed16cd14f926b3304d7d3bd53dd79))

- Design for VFS shadow (scheme file) + (scheme show), closes #91 ([`4b6d4e8`](https://github.com/emesal/tein/commit/4b6d4e8a5af02af5af32ee3d1dd87207423d6d44))

- Mark (scheme file) shadow design as blocked on VFS refactor ([`e471184`](https://github.com/emesal/tein/commit/e471184ea57be63a76f296e1a0a1a99221812d18))

- Design for VFS module registry refactor, closes #95 ([`dc2321a`](https://github.com/emesal/tein/commit/dc2321a3a6a5c7436a75b381e111f44e1228b9b6))

- Finalise VFS registry refactor design ([`0a2d448`](https://github.com/emesal/tein/commit/0a2d4483b0d2c2f5062f9729d29dc3ac33df45d1))

- Modules::only() constructor fn instead of variant ([`ee10b47`](https://github.com/emesal/tein/commit/ee10b471fa46f0a9b1de9f15d197a097ed73cef1))

- Refine VFS registry refactor design after review ([`af4824b`](https://github.com/emesal/tein/commit/af4824b2557a27b767d3439fc65d2b6cceb42312))

- Implementation plan for VFS registry refactor ([`624821a`](https://github.com/emesal/tein/commit/624821af44d255d729ad327278610505cb227e3e))

- Implementation plan for (scheme file) VFS shadow + (scheme show) in Modules::Safe ([`79fc007`](https://github.com/emesal/tein/commit/79fc00758ae6e59dc04c2bf8997bbc24ea9cdafd))

- Handoff notes for vfs-shadow-scheme-file-2603 batch 1 ([`742b7d1`](https://github.com/emesal/tein/commit/742b7d1825daed843da1e7566572eae48971cfa7))

- Update handoff notes for batch 2 (task 6 complete, arch discoveries) ([`dfbce44`](https://github.com/emesal/tein/commit/dfbce44b1bcd1a97097bf676bba7b9d66a77e21d))

- Update handoff notes for batch 3 (task 7 complete, task 8 blocked) ([`cddb0c1`](https://github.com/emesal/tein/commit/cddb0c1decdbe306e6432ed187b570d7bdba735f))

- Update handoff notes for batch 4 (task 8 resolved, GH #98 for clib audit) ([`9947aee`](https://github.com/emesal/tein/commit/9947aeee1000d5c667c1a4b9ced344b37088b044))

- Update plan + design doc progress, fmt build.rs ([`77c9db7`](https://github.com/emesal/tein/commit/77c9db75a308c7fa6fa1104196efddefda788a69))

- Design for C-level FsPolicy enforcement in chibi opcodes ([`241d401`](https://github.com/emesal/tein/commit/241d401bbd5fdfc60ebe5ad9b18a0cd7b63f35a1))

- Implementation plan for C-level FsPolicy enforcement ([`2ba8f5f`](https://github.com/emesal/tein/commit/2ba8f5f426cb66d89215dd3bd8c09e8afc2a5655))

- Update implementation plan progress (tasks 1-7 complete) ([`03e63d5`](https://github.com/emesal/tein/commit/03e63d5fb4af90d8d52f7308b61a129108ab0037))

- Update AGENTS.md + sandbox docs for C-level FsPolicy enforcement ([`600fe2b`](https://github.com/emesal/tein/commit/600fe2b7a5e1cb0b13cf9f029c2640736136286e))

- Update implementation plan progress (tasks 1-10 complete) ([`6058eec`](https://github.com/emesal/tein/commit/6058eecb63a3b080a05794987bb5ebe727797639))

- Fix corrupted docstring on register_file_module ([`91b8364`](https://github.com/emesal/tein/commit/91b8364178c3db2aba56e882a17184ef5ae7ce3c))

- Fix misleading inline comment — shadow injection is before VFS gate, not FS gate ([`22d551b`](https://github.com/emesal/tein/commit/22d551b0692af69cbcf9e7d8d2dea11344c9ac3a))

- Fix stale comment in test_open_input_file_unsandboxed_passthrough ([`0d647d3`](https://github.com/emesal/tein/commit/0d647d3c909caa6222eb139631c35158c3f36e03))

- Update sandboxing flow in AGENTS.md — add shadow injection step ([`bce414e`](https://github.com/emesal/tein/commit/bce414ea3a2c02121407832128ceedd76e33e6a2))

- Document file_read/file_write + exit r7rs deviation (GH #101) ([`74af014`](https://github.com/emesal/tein/commit/74af0141acaff33df4f498ba83770f01c7a6bbbf))

- Design doc for full docs restructure + README/ARCHITECTURE/ROADMAP sync ([`4cf9e9b`](https://github.com/emesal/tein/commit/4cf9e9b8dfcd74bb8edb59e771befa8ebf88ec9b))

- Implementation plan for full docs restructure ([`5255635`](https://github.com/emesal/tein/commit/525563544e8cef175bfdb1d489ccd96074a49860))

- Rewrite README — lean landing page, remove roadmap section ([`8127a63`](https://github.com/emesal/tein/commit/8127a63acdf5d245396dcde0294609f2ad4c6ce6))

- Mention TimeoutContext alongside ThreadLocalContext in README features ([`9400d5a`](https://github.com/emesal/tein/commit/9400d5add1191af9e454d4f333c2fcefbc021d45))

- Add quickstart.md ([`83f1774`](https://github.com/emesal/tein/commit/83f177400124b39859e294ee61cb67a2486b10af))

- Fix Context::new() description and extraction helper lifetimes in quickstart ([`202fb92`](https://github.com/emesal/tein/commit/202fb9289f33c04f249444d898c09ffc9cb1a16c))

- Add embedding.md — context types, Value enum, builder API, ports ([`303dd04`](https://github.com/emesal/tein/commit/303dd04585ff8537abfbebb55f2c8341479a678c))

- Fix stale variable name in output port example ([`b44b19d`](https://github.com/emesal/tein/commit/b44b19da38aeb82858f3cefcd4bff396c98c4a8f))

- Add sandboxing.md — four-layer model, Modules, FsPolicy, timeout ([`6a0cdfd`](https://github.com/emesal/tein/commit/6a0cdfdd02d3963692419da166a9dc545fdfbb4d))

- Update plan progress (tasks 1-5 complete) ([`21f985d`](https://github.com/emesal/tein/commit/21f985d27ab489d23b3d6009a7284909b7c20fd1))

- Add rust-scheme-bridge.md — tein_fn, tein_module, ForeignType, reader/macro hooks ([`1df0c84`](https://github.com/emesal/tein/commit/1df0c8485bcc506b1e5a95d767e4bcbe79be28f4))

- Add modules.md — (tein json/toml/uuid/time/process/file/docs/load) ([`f6a73e8`](https://github.com/emesal/tein/commit/f6a73e8e8e0b10b8a7260ca0f0b99c0118eabc65))

- Add extensions.md — cdylib extension system, tein-ext, stable ABI ([`e6da196`](https://github.com/emesal/tein/commit/e6da196f72bb0bf7b9b0fcda08cae3cfa5122023))

- Update plan progress (tasks 6-8 complete) ([`dd0adc4`](https://github.com/emesal/tein/commit/dd0adc416e33e87a71e54eba6b36cddec1803848))

- Add tein-for-agents.md — sandbox model, LLM-navigable errors, agent design ([`01209c2`](https://github.com/emesal/tein/commit/01209c2fca7f6a5377c6b7a706a78c79b77e2c5f))

- Add reference.md — Value types, feature flags, VFS modules, env quirks ([`97afe0f`](https://github.com/emesal/tein/commit/97afe0f3a1e0937244a6f6ed502f8fc3a19ae850))

- Rewrite guide.md as index/TOC — links all new docs/ ([`86713db`](https://github.com/emesal/tein/commit/86713db163f00da3cf2b468a95f01c9df9e1ff98))

- Update plan progress (tasks 9-11 complete) ([`3b22dad`](https://github.com/emesal/tein/commit/3b22dad842c7ab24df73ea6360e72d9a0f63d206))

- Update ARCHITECTURE.md — M8 status, src files, eval.c patches, new flows, docs/ note ([`fa46eef`](https://github.com/emesal/tein/commit/fa46eefda6599e294e1bb66ca5c4e5113d5ccdd4))

- Update ROADMAP.md — move shipped M8 items to completed, list remaining M8 work ([`b5a9981`](https://github.com/emesal/tein/commit/b5a9981b1be00fd0621d7ad8a3f965b991c0a906))

- Update plan progress (tasks 12-13 complete) ([`bac0760`](https://github.com/emesal/tein/commit/bac0760ca7bc733f38968690d8840aee61992c83))

- Update plan progress (task 14 complete — PR #102) ([`aaa6fe1`](https://github.com/emesal/tein/commit/aaa6fe17bacb8a5bf74000cc74d0234ca0c3de5f))

- Update module inventory handoff (session 2 complete) ([`8006082`](https://github.com/emesal/tein/commit/8006082042b1dab4c26c0a1d29a6bafdfc804e0c))

- Update handoff — session 3 progress, shadow module design next ([`892696a`](https://github.com/emesal/tein/commit/892696a0176faa0baf73ffc137bbaa2d7df78a9a))

- Shadow module stubs design — data-driven build-time generation ([`da59012`](https://github.com/emesal/tein/commit/da590125384150517181d3822948599427b13d0b))

- Shadow module stubs implementation plan — 8 tasks ([`79cd055`](https://github.com/emesal/tein/commit/79cd0558c82834fe3755300106c16c80b4d2abdf))

- Update handoff — shadow stubs complete (session 4), document gating roadmap ([`29e2df2`](https://github.com/emesal/tein/commit/29e2df2e6a97ef3e0c153711aacffab59440db6c))

- Update module inventory — shadow stubs + chibi/channel marked done ([`fadb11e`](https://github.com/emesal/tein/commit/fadb11e8ce4e9be0a4af9fa65b204b59fb61b8ae))

- Sync module inventory — sessions 2-4 additions + plan marked complete ([`1cbbc08`](https://github.com/emesal/tein/commit/1cbbc08ec0c2d16002ecaf0dde84401ada2e07c4))

- Fix handoff session count (three → four) ([`3ac956e`](https://github.com/emesal/tein/commit/3ac956e8b6af9aed7af9a101c055617b0eb0a620))

- Update module inventory — session 5 additions + remaining ❌ triage ([`c5f4740`](https://github.com/emesal/tein/commit/c5f47408882e0ec92a6044b4a3e4779716eb2880))

- Module inventory completion design + appendices A/B ([`447c675`](https://github.com/emesal/tein/commit/447c675855b966c44ade28b887f1b3e1217e8419))

- Module inventory completion implementation plan ([`848f212`](https://github.com/emesal/tein/commit/848f212dd600ec6ab43ef305f701fd042361160e))

- Update plan — batch 4 bootstrap notes for context clear ([`1efa57a`](https://github.com/emesal/tein/commit/1efa57aafdc5709b91e6bf6bed885cbc6e1b620e))

- Finalise module inventory summary — all modules resolved ([`d118de6`](https://github.com/emesal/tein/commit/d118de67efed4c49aa36f0be2e81506a0b8b3905))

- Complete module inventory — close #92 ([`fe20398`](https://github.com/emesal/tein/commit/fe2039888d407eed2bebc3e4e32e727992182992))

- Remove stale AGENTS.md note — Modules::Safe includes tein/process ([`60e1b08`](https://github.com/emesal/tein/commit/60e1b0804f5b29c461180a77989f7d9d707af231))

- Cross-reference duplicated feature_enabled in build.rs + sandbox.rs ([`85721ba`](https://github.com/emesal/tein/commit/85721bac0525d67235fb490bb81eada5f29ace52))

- Clarify binary-record-chicken.scm in VFS registry — never loaded by chibi ([`6340c6e`](https://github.com/emesal/tein/commit/6340c6e02e752f8b0fbac8ab6da0940dfc1e404d))

- Clarify allow_module dep resolution happens at build() time ([`b6c85ed`](https://github.com/emesal/tein/commit/b6c85edd78bdacd881f496afa0f41098474b3d6e))

- Add comments explaining chibi/channel and scheme/mapping/hash safety rationale ([`2f57507`](https://github.com/emesal/tein/commit/2f57507e6e30aa0401ab742d68d8930f6e0fa647))

- Document cond-expand limitation in collect_exports_from_sexps ([`abd907f`](https://github.com/emesal/tein/commit/abd907f4192e9bb5d395c6bb1374922a5e313eb1))

- Implementation plan for fix include-shared stub modules (#103) ([`25eee13`](https://github.com/emesal/tein/commit/25eee134a87603a4ec953c8ce372bcf80e9bfcad))

- Design for fix include-shared stub modules (#103) ([`df494b4`](https://github.com/emesal/tein/commit/df494b4d5f64074bece0a266becd9a6859ceab9b))

- Design for extended module testing via chibi/test + srfi/N/test suites ([`cea7b49`](https://github.com/emesal/tein/commit/cea7b49f03c06da7a5c03ba526ad10e573ec65b7))

- Extended module testing implementation plan ([`30d9d4b`](https://github.com/emesal/tein/commit/30d9d4b270819cb5441ae10765b19b3a84183a74))

- Update plan with task 1-3 completion + task 4 blocker analysis ([`43fad9f`](https://github.com/emesal/tein/commit/43fad9fb927242451cc4dd33be0b9b726183402a))

- Update AGENTS.md — test counts, vfs harness notes, shadow SLD rules ([`d12fe60`](https://github.com/emesal/tein/commit/d12fe603f22dc71b0f021cc536d8a856f1058d79))

- Srfi-19 time data types design ([`993b2b4`](https://github.com/emesal/tein/commit/993b2b4ba3c3315be5e6a0c3c1945213012b1671))

- Srfi-19 implementation plan ([`06b0f52`](https://github.com/emesal/tein/commit/06b0f52484c4db38d59e53bcba9a45168be1481f))

- Update srfi-19 plan — tasks 5-7 complete, tasks 8-9 documented ([`9810382`](https://github.com/emesal/tein/commit/9810382b999ea51b56be04591048b9115c141595))

- Add (srfi 19) to reference, module-inventory, sandboxing; AGENTS.md gotchas ([`9668334`](https://github.com/emesal/tein/commit/9668334ce67d29688900a4fb8d73f4b25a336fb4))

- Add design for sandbox fake env vars + command-line (#99) ([`bc800ce`](https://github.com/emesal/tein/commit/bc800ceef8d29fad99f38362cd6d9033d71301d3))

- Add implementation plan for sandbox fake env vars (#99) ([`218ad1e`](https://github.com/emesal/tein/commit/218ad1e19e8ff4dc53686c60ea1923c98d0d477c))

- Update sandbox docs for fake env vars + command-line (#99) ([`ffbbbaa`](https://github.com/emesal/tein/commit/ffbbbaa0b3424842bbfa88d927c7b36ccc93c333))

- Update AGENTS.md with sandbox env/command-line notes; fix shadow stubs ([`77ba3a3`](https://github.com/emesal/tein/commit/77ba3a30d322f32fb6aed6192b8d09204fac1b07))

- Design for (tein safe-regexp) #37 ([`1878a3f`](https://github.com/emesal/tein/commit/1878a3f28e32e34f69fdf45d4230a8e6f653c75f))

- Implementation plan for (tein safe-regexp) #37 ([`21e2b75`](https://github.com/emesal/tein/commit/21e2b75d6fd94dceec7901df40516f7e850d2d1d))

- Revised plan — foreign type + macro fix (#37, #114) ([`c4a5bd9`](https://github.com/emesal/tein/commit/c4a5bd99a14d610573c71981e193e3cb5fb1cf3a))

- (tein safe-regexp) in AGENTS.md and reference.md (#37) ([`cc9f240`](https://github.com/emesal/tein/commit/cc9f2400be4b0a7399da56346fe277883742083e))

- Update safe-regexp plan — all tasks complete ([`343c299`](https://github.com/emesal/tein/commit/343c29903bbf29d3777e9c1a72c6ca1563f04926))

- Tein binary design — standalone scheme interpreter/REPL (#42) ([`b13312b`](https://github.com/emesal/tein/commit/b13312bcc4acf5a77231c9dd37dd98e70fc40c49))

- Tein binary implementation plan (#42) ([`b9d25f3`](https://github.com/emesal/tein/commit/b9d25f3ccf9ce2ff7b8cb01c3712d6a06acecd3b))

- Document tein binary, update AGENTS.md commands and test counts ([`6b5a4b1`](https://github.com/emesal/tein/commit/6b5a4b10bfe81d8ac354e07a102452e687031f60))

- Design for (tein crypto) hashing + CSPRNG module (#38) ([`93edf41`](https://github.com/emesal/tein/commit/93edf41339e99dbd92419c7c5b5518a09583d955))

- Implementation plan for (tein crypto) (#38) ([`d6a5d33`](https://github.com/emesal/tein/commit/d6a5d333913c767d17478d425b375bf4bb0fc5f1))

- **plan:** Set_current_port API + REPL TrackingWriter design ([`b04f91c`](https://github.com/emesal/tein/commit/b04f91c2c6d37c95149f40d7c93f33b069c1d3a1))

- **plan:** Set_current_port implementation plan ([`f18eb08`](https://github.com/emesal/tein/commit/f18eb08324bbfbf3acacd1b6ed813a74f04fc30a))

- Add set_current_*_port to embedding guide + reference ([`4efb2ba`](https://github.com/emesal/tein/commit/4efb2baa1837412dc91b715ea7c45ff41e7b2ebe))

- Design for chibi regexp VFS smoke tests (#85) ([`cdd394b`](https://github.com/emesal/tein/commit/cdd394b181a72a640f3de8849b3d110b6fe7d6ae))

- Implementation plan for chibi regexp VFS smoke tests (#85) ([`7194b96`](https://github.com/emesal/tein/commit/7194b96c386f9e92b485e528d1cc00cfb24f67f4))

- Document (chibi regexp) sandbox gating and ReDoS caveat (#85) ([`2e63f97`](https://github.com/emesal/tein/commit/2e63f9782cb53aa9a3b140820e809ca9e2a9d334))

- Design for sandboxed (scheme eval) + (scheme load) + (scheme repl) (#97) ([`f1af097`](https://github.com/emesal/tein/commit/f1af097d2242cb9b6b6528a7587bde253c548bf8))

- Implementation plan for sandboxed eval/load/repl (#97) ([`fe8e087`](https://github.com/emesal/tein/commit/fe8e087a56fac2b0924109aab68f446d6387797c))

- Handoff notes for sandboxed eval continuation (#97) ([`624e4e0`](https://github.com/emesal/tein/commit/624e4e0aa678f40cbc2fdf85b0e4c269ed4b6232))

- **agents:** Add sandboxed eval/load/repl flow + shadow list (#97) ([`e9a6ddd`](https://github.com/emesal/tein/commit/e9a6dddcd4d4f21b8ba7c081939325ddfb16b44d))

- **sandbox:** Update shadow module comment block (#97) ([`a426fbd`](https://github.com/emesal/tein/commit/a426fbdc132a846fbabf2f249109c1d26245a2f2))

- **design:** Dynamic module registration (#132) ([`070cf3b`](https://github.com/emesal/tein/commit/070cf3b9b9af2e919a12f63b88a343b2d71396f2))

- **plan:** Implementation plan for dynamic module registration (#132) ([`23c9efe`](https://github.com/emesal/tein/commit/23c9efe17795ae5c6d93ce94d5cecbbace0f3017))

- **plan:** Revise impl plan — CONTEXT_PTR eliminates parsing duplication (#132) ([`e2ca3ae`](https://github.com/emesal/tein/commit/e2ca3ae1f773c0370ed9e665b16a5489c9cade40))

- Update AGENTS.md and design doc for dynamic module registration (#132) ([`fab1deb`](https://github.com/emesal/tein/commit/fab1deb6435694dd2cdd6efc7412481edbff5a9f))

- Design for (tein http) module (#130) ([`8c034bb`](https://github.com/emesal/tein/commit/8c034bb9de2c0ab6d552aaffe6d10de785e13fd9))

- Implementation plan for (tein http) module (#130) ([`5048f67`](https://github.com/emesal/tein/commit/5048f67e3ea3bdd6b3f9cce11d90b2c6c03b20e6))

- Update AGENTS.md for (tein http) module (#130) ([`dbafd3a`](https://github.com/emesal/tein/commit/dbafd3a205ae5bb6ad0da55c7c797b41d1b5a40a))

- Add (tein http) to reference docs (#130) ([`0962d27`](https://github.com/emesal/tein/commit/0962d2727114e61b82dac5832317b36459167057))

- Add design for Result::Err → r7rs exceptions (#135) ([`aeb1e3c`](https://github.com/emesal/tein/commit/aeb1e3cca2ac290b886813181bcb600923030869))

- Add implementation plan for Result::Err → exceptions (#135) ([`5639357`](https://github.com/emesal/tein/commit/5639357a49c4aef83d5a94d6f0316141922349ca))

- Update error handling convention — exceptions not strings (#135) ([`01ab3a2`](https://github.com/emesal/tein/commit/01ab3a28c445d37ef91ec7ed4e272c4f64a6de4f))

- Design doc for filesystem module search path (#131) ([`aa7c24f`](https://github.com/emesal/tein/commit/aa7c24f01d775a0cbd3f9c3beb0a7280d4e65dbc))

- Implementation plan for filesystem module search path (#131) ([`417ef2d`](https://github.com/emesal/tein/commit/417ef2de3aff6e5f1774fcadd71efdec78fa83b4))

- **agents:** Document FS_MODULE_PATHS and TEIN_MODULE_PATH (#131) ([`f90cf64`](https://github.com/emesal/tein/commit/f90cf64751972cae29926b03da4ad5eaf32ddee6))

- Design for r7rs-compliant exit with dynamic-wind cleanup (#101) ([`0bcb275`](https://github.com/emesal/tein/commit/0bcb27583a792b65c20c03c889bf27be56bc814e))

- Implementation plan for r7rs exit with dynamic-wind (#101) ([`404f16c`](https://github.com/emesal/tein/commit/404f16ca19a29a54ea6ba4f9287e0da8e85e0f9f))

- **agents:** Update exit escape hatch flow for r7rs compliance (#101) ([`452c204`](https://github.com/emesal/tein/commit/452c20457137703c3775014986ae5b76f0a4c313))

- Update architecture and reference for r7rs exit compliance (#101) ([`e3d9634`](https://github.com/emesal/tein/commit/e3d96347806d5c19ecfa39ac7c39c461d1df1cde))

- **examples:** Update sandbox example for auto-import ([`3fcb2ac`](https://github.com/emesal/tein/commit/3fcb2ac77b8db86c818c70c5099715f17d0f675c))

- Document sandbox auto-import of scheme/base and scheme/write ([`edc9cb4`](https://github.com/emesal/tein/commit/edc9cb4304861da2c94c3cf9f60bfdfd387871f8))

- **plans:** Add sandbox auto-import design and implementation plan ([`6c7f5df`](https://github.com/emesal/tein/commit/6c7f5df8d34b90a578e577acc6f581217081e441))

- **plans:** Universal module availability design ([`3d4060f`](https://github.com/emesal/tein/commit/3d4060f67cb775f3a39be01ecf5f4dc534a0860d))

- **plans:** Universal module availability implementation plan + handoff ([`8f67112`](https://github.com/emesal/tein/commit/8f67112e658395c4f5a553fb2ece28930ef1e5cb))

- Amend plan — task 6 must update (tein file) to import from (tein filesystem) ([`a540db3`](https://github.com/emesal/tein/commit/a540db3d30ddd86dc78f1f5fa4bab4baea8450fb))

- Update handoff + plan to reflect tasks 1-3 complete ([`4c38f5b`](https://github.com/emesal/tein/commit/4c38f5b928b23aa1a9de7adbf66dc11b8cf768b2))

- Update AGENTS.md for universal module availability ([`3ada205`](https://github.com/emesal/tein/commit/3ada205f785c4379881235973bcf3f0bc7a3f2ba))

- Fix stale shadow SLD references in vfs_registry comments ([`19d18fd`](https://github.com/emesal/tein/commit/19d18fdf4f0493bf392185590bd4742949ecee3a))

- Update CHANGELOG for v0.2.3 ([`3089149`](https://github.com/emesal/tein/commit/3089149b64a2ea65b5ff81c73bb94ca53a788d8f))


### Features

- Add Char, Bytevector, Port value types (milestone 4b) ([`694a7d7`](https://github.com/emesal/tein/commit/694a7d75350f08cf65a5537dd99fd3d8261efed7))

- Sandbox-aware error messages via Error::SandboxViolation ([`5b5e88b`](https://github.com/emesal/tein/commit/5b5e88b12f01f9040a3485ac7c529ca1129df491))

- **repl:** Add paren-depth balancing helper with tests ([`1d0a806`](https://github.com/emesal/tein/commit/1d0a80625ed9aaf1f08424cb5e394c05829ffaad))

- **repl:** Interactive scheme REPL with rustyline ([`a0a4171`](https://github.com/emesal/tein/commit/a0a4171c9d4cbbecb63edb50b350e6c8067d0012))

- **serde:** Implement Serialize/Deserialize for Sexp ([`4e7e6d4`](https://github.com/emesal/tein/commit/4e7e6d45225a537427db1115e9fc62b8840e5d4f))

- **serde:** Add from_reader, to_writer, to_writer_pretty ([`b1e1604`](https://github.com/emesal/tein/commit/b1e16045f304af0daab52ce38a41ed97f97bb562))

- Derive Clone on ContextBuilder ([`362479a`](https://github.com/emesal/tein/commit/362479a52fde1965ae871ee5bf2062cee41d15ab))

- Add ThreadLocalContext with persistent/fresh modes and reset ([`4e1d9b9`](https://github.com/emesal/tein/commit/4e1d9b9ebf749666b3ec5a2d9e15119cbd11aa12))

- Add custom ports and reader dispatch extensions ([`da8bff1`](https://github.com/emesal/tein/commit/da8bff124cdb69ad8cd709808d6f5d176692a59a))

- Add macro expansion hook thread-locals to tein_shim.c ([`8597514`](https://github.com/emesal/tein/commit/8597514ebc633d71b811e74a9a2888d132d8d2d5))

- Patch eval.c to call macro expansion hook after expansion ([`2fdca6d`](https://github.com/emesal/tein/commit/2fdca6d71f4421ca96c20e6070d4b1ec28b2070d))

- Add macro expansion hook FFI bindings ([`85b3300`](https://github.com/emesal/tein/commit/85b3300e850b37a8678a5c3ed3254940726a24ba))

- Add macro expansion hook wrappers, registration, cleanup ([`6c36737`](https://github.com/emesal/tein/commit/6c367376a9cd4ff4b426fb8ee0a3c3039726e8de))

- Add (tein macro) VFS module for macro expansion hooks ([`02ca927`](https://github.com/emesal/tein/commit/02ca927ad35663e4a746987a065e8dff9e764069))

- Add debug-chibi cargo feature for GC instrumentation ([`0b8d56f`](https://github.com/emesal/tein/commit/0b8d56f7df44769d6762d375f1444d90ba09e17b))

- Context::register_vfs_module — runtime VFS registration from rust ([`246bdb3`](https://github.com/emesal/tein/commit/246bdb34268c8f690c309e01c4f80afda2e505eb))

- #[tein_fn] replaces #[scheme_fn] — same codegen, unified naming ([`652d5f4`](https://github.com/emesal/tein/commit/652d5f4972d788743a2637241a47e2480f873a54))

- Naming helpers + #[tein_module] scaffold (tasks 4–6, 8 codegen) ([`f6ace94`](https://github.com/emesal/tein/commit/f6ace94b46d29ed7e11394bfca02ff632e960da4))

- Complete #[tein_module] system — tests, cleanup, docs (tasks 9-12) ([`69eb45a`](https://github.com/emesal/tein/commit/69eb45a2ae935f8697064fa0cca325dc709fa813))

- Add #[tein_const] to #[tein_module] ([`d2e5bc9`](https://github.com/emesal/tein/commit/d2e5bc94d08a8a317f85652c56ec4916c9f37771))

- **macros:** Add extract_doc_comments helper (#60) ([`e51d009`](https://github.com/emesal/tein/commit/e51d00989b5579bc621d35743a43e8dcfe6032aa))

- **macros:** Add doc field to all info structs (#60) ([`bc35f8e`](https://github.com/emesal/tein/commit/bc35f8ef5f71ab75e86bae0434a877bfb692296c))

- **macros:** Emit ;; doc comments in generated .scm (#60) ([`f5300f5`](https://github.com/emesal/tein/commit/f5300f5e3570433a9d7536aa1391226b4ebe5784))

- (tein docs) + per-module doc sub-libraries (#61) ([`20e38a3`](https://github.com/emesal/tein/commit/20e38a37b9144661f609b8efe724958be3998c8a))

- Cdylib extension system — tein-ext, load_extension, ForeignStore ext types (#62) ([`a7c855b`](https://github.com/emesal/tein/commit/a7c855ba67a713bf0f8c5e133ed6a9abce73450e))

- **tein-macros:** Parse ext = true flag in #[tein_module] (#62) ([`4e18b54`](https://github.com/emesal/tein/commit/4e18b5493f50c47684cfdcf846086d7f1e6f3a58))

- **tein-macros:** Ext-mode codegen for #[tein_module] (#62) ([`2a29d0c`](https://github.com/emesal/tein/commit/2a29d0caf17033c1086e9cdc605513d1ae1de69c))

- **tein-test-ext:** In-tree test cdylib extension (#62) ([`1c656fd`](https://github.com/emesal/tein/commit/1c656fd690910b9c90b6fb0cab8ea72a3e462647))

- **ffi:** Safe wrappers for numeric tower — bignum, rational, complex (#71) ([`37e7e05`](https://github.com/emesal/tein/commit/37e7e059563bb2cff5b15b519c106d9a73dbf4a0))

- **value:** Add Bignum, Rational, Complex variants with from_raw/to_raw, Display, PartialEq, accessors, and tests (#71) ([`43df9a0`](https://github.com/emesal/tein/commit/43df9a06696663c05d3e7cd49a0cd77d4780f884))

- **tein-sexp:** Add Bignum, Rational, Complex, Bytevector to SexpKind (#71) ([`2dd42f3`](https://github.com/emesal/tein/commit/2dd42f345bacd0ac5d1c8ee414cc03a80e78162a))

- **tein-sexp:** Lexer/parser for bignum, rational, bytevector literals (#71) ([`603df68`](https://github.com/emesal/tein/commit/603df681dfcddb859c7227001014ef8ed35e6222))

- **tein-sexp:** Complex number lexing and parsing (#71) ([`ff9f5a1`](https://github.com/emesal/tein/commit/ff9f5a136549a854557b85ddb401d403f1d51dee))

- **json:** Add serde_json and tein-sexp dependencies ([`ca14e68`](https://github.com/emesal/tein/commit/ca14e68f6af81ebbe47e43e3e0f62046c2b2f6d6))

- **json:** Add Value <-> Sexp bridge module ([`acd0c99`](https://github.com/emesal/tein/commit/acd0c99eca6a2f75d2571c8f0ea0a9d1524c1b48))

- **json:** Add json parse/stringify module ([`41c165c`](https://github.com/emesal/tein/commit/41c165c52990a6c57dea15653c8fbbd95b12d6c1))

- **json:** Register json-parse/json-stringify trampolines + VFS module ([`397e48d`](https://github.com/emesal/tein/commit/397e48d6864aecdda11e0378b29f25cb0d49bbfe))

- Feature-gate json module behind `json` cargo feature ([`b850cc9`](https://github.com/emesal/tein/commit/b850cc90457c5fd72f5f510961f5b2b7cfac6836))

- Add toml crate dependency behind `toml` cargo feature (#77) ([`3916181`](https://github.com/emesal/tein/commit/39161810b60fb25e1d0ba4cfaa8284236ef213bc))

- **toml:** Add toml_parse — TOML string to scheme Value (#77) ([`16143fa`](https://github.com/emesal/tein/commit/16143fabf3ad1749df7802c852b40563ada546b8))

- **toml:** Add toml_stringify_raw — scheme Value to TOML string (#77) ([`6dc3ea8`](https://github.com/emesal/tein/commit/6dc3ea828f69920a6fcc57c83b47ca977a3fc154))

- **toml:** Add trampolines and registration in context.rs (#77) ([`1bdc07b`](https://github.com/emesal/tein/commit/1bdc07bfe8f2a2dc264c258a0db611467803bcfb))

- **toml:** Add VFS module files and build.rs gating (#77) ([`fc68df1`](https://github.com/emesal/tein/commit/fc68df118ef27397ab9859350aa996eed3c13bbf))

- **toml:** Add integration tests — scheme and rust (#77) ([`be5e0ae`](https://github.com/emesal/tein/commit/be5e0ae8b84acd31216956c8ec5473d353fbaae3))

- **sandbox:** Rework ModulePolicy to three-tier Allowlist/VfsAll/Unrestricted (#86) ([`5a4a026`](https://github.com/emesal/tein/commit/5a4a02617efaf776b2b5ddf8e0ea056f89301971))

- Wire up three-tier module policy across C/rust boundary (#86) ([`ef17ef9`](https://github.com/emesal/tein/commit/ef17ef9e666a9ad2ad3953a5efe8055c84c4c1ef))

- Expand SAFE_MODULES to full safe r7rs subset (#86) ([`10c5548`](https://github.com/emesal/tein/commit/10c5548ca5df863395171be12609cb6700564d81))

- Add scheme/eval and scheme/repl to SAFE_MODULES (#86) ([`06b4a81`](https://github.com/emesal/tein/commit/06b4a8199af762c1c142867464b711695d54fbda))

- Replace tein/ blanket with explicit SAFE_MODULES entries (#87) ([`fa6cbee`](https://github.com/emesal/tein/commit/fa6cbee2df7f2e2f66034a0a1091a7705741ceee))

- Expose tein_vfs_lookup in ffi.rs (#87) ([`9aa29f6`](https://github.com/emesal/tein/commit/9aa29f6a31222c9c01ae10984627cf5741abd1dd))

- Add exit escape hatch thread-locals and eval intercept (#87) ([`5203d14`](https://github.com/emesal/tein/commit/5203d143f4949181f0a22e5b9fd08d3b9347093e))

- Add VFS module files for (tein file), (tein load), (tein process) (#87) ([`943ab8f`](https://github.com/emesal/tein/commit/943ab8f5d834fe32713bec1e7a917c92413efe1f))

- Add (tein file/load/process) to VFS build (#87) ([`3948c82`](https://github.com/emesal/tein/commit/3948c820908fba19c25fec21333710169cfc8342))

- Implement (tein file), (tein load), (tein process) trampolines (#87) ([`127a96c`](https://github.com/emesal/tein/commit/127a96c684e7f3d23a0d7bb96eb832fca2407eb6))

- Support Value arg type in #[tein_fn] free fns ([`f0c5d86`](https://github.com/emesal/tein/commit/f0c5d86d7567608acad93c6a887fd22bb0eff0b9))

- Implement (tein uuid) module ([`e20a971`](https://github.com/emesal/tein/commit/e20a9711fb259ac6736cde833b45f8c87b10c6dc))

- Register (tein uuid) in standard env, add to SAFE_MODULES ([`39d81bc`](https://github.com/emesal/tein/commit/39d81bc8e0736e69ed31b99e15096dad7454b646))

- **time:** Add (tein time) module implementation (#90) ([`3d537c3`](https://github.com/emesal/tein/commit/3d537c3aaa10e3143c77e27ae57abba5c951bddc))

- **time:** Wire up feature gate and context registration (#90) ([`c0e819a`](https://github.com/emesal/tein/commit/c0e819a85ce8960070af9b2fa794ff2bbddc9427))

- **time:** Add (tein time) to VFS_MODULES_SAFE (#90) ([`5de58cb`](https://github.com/emesal/tein/commit/5de58cb27499aebb3e5a3d2f6c8c5ea362ee0eca))

- Add vfs_registry.rs with VfsEntry and full module registry ([`5b4decd`](https://github.com/emesal/tein/commit/5b4decd841fd030d2c6d506eb96977b4bfdcb59a))

- Wire VFS_REGISTRY into sandbox.rs with registry helper functions ([`e50cb2e`](https://github.com/emesal/tein/commit/e50cb2e3e5a31004f3298f63e6713f6b62c0af95))

- Wire VFS_REGISTRY into build.rs, add SLD include validation ([`a8eaf25`](https://github.com/emesal/tein/commit/a8eaf254a2f5cfb0cb2f1d67a082215f1792763c))

- Add export extraction to build.rs, generating tein_exports.rs ([`d319b47`](https://github.com/emesal/tein/commit/d319b47a0759feb03218e8d24c2ef7d050e72c0e))

- Include generated exports in sandbox.rs, add module_exports/unexported_stubs ([`f39cf6c`](https://github.com/emesal/tein/commit/f39cf6c2b399788582fcde234dfa0823ffd19a2c))

- Add Modules enum and sandboxed() builder method (task 6) ([`ecca097`](https://github.com/emesal/tein/commit/ecca097d28667c0d29098ef54e1160c146106ec0))

- Implement registry-based sandbox build path (task 7) ([`4f5fcc7`](https://github.com/emesal/tein/commit/4f5fcc79a5c0278a41972195cbf8e8637ace97b2))

- Integrate file_read/file_write with sandboxed() path (task 8) ([`7143212`](https://github.com/emesal/tein/commit/7143212df1f761129c257aff3d876fe542495b28))

- Update allow_module() to use registry deps in sandboxed() path (task 9) ([`7b1cdd8`](https://github.com/emesal/tein/commit/7b1cdd81446f944eb1835fca5769a410f07c3f94))

- Remove old preset system, update all tests to new Modules API (tasks 10-11) ([`935e37c`](https://github.com/emesal/tein/commit/935e37c5844a097c5eb6951731788b0548c289cf))

- Update sandbox example, docs, and AGENTS.md for new Modules API (task 12) ([`97726bd`](https://github.com/emesal/tein/commit/97726bdcaaa5bbf0bc0dd9d8bdade7336e90ca47))

- Add VfsSource::Shadow variant + scheme/file and scheme/repl shadow entries ([`17d9fb7`](https://github.com/emesal/tein/commit/17d9fb7c6c484b74ec77ceefa688c2ba2e80ad60))

- Register_vfs_shadows() — data-driven shadow injection from VFS_REGISTRY ([`79ff3dd`](https://github.com/emesal/tein/commit/79ff3dd87e0cb440b3252e873afb2768c4d48d0d))

- Add open-*-file trampolines + capture_file_originals for (tein file) ([`0f7caa3`](https://github.com/emesal/tein/commit/0f7caa3c24d4a49e7e95f96539fcdc80741ff21e))

- Add open-*-file trampolines + capture_file_originals + higher-order wrappers for (tein file) ([`414d834`](https://github.com/emesal/tein/commit/414d83466e7e7ab331042234e24fae7d12116545))

- Add missing clibs + sandbox-safe (tein process) + shadows for srfi/98 & scheme/process-context ([`3a80842`](https://github.com/emesal/tein/commit/3a8084205db0e5a6ccb19c739c63c9e755121625))

- Tein_fs_policy_check C→rust callback + FFI wrappers ([`1aa31f6`](https://github.com/emesal/tein/commit/1aa31f6af3efc0d473e730dfb7f6e49c811f45c4))

- FS_GATE thread-local — arm in sandboxed build, clear on drop ([`eee0e6b`](https://github.com/emesal/tein/commit/eee0e6b83a111db609fe414ff341cc3bd50c5ae1))

- Add 47 pure-scheme modules to VFS registry + module inventory ([`192fbe2`](https://github.com/emesal/tein/commit/192fbe2f014844f22088b92aab2514acd6c835c2))

- Add chibi/crypto/*, chibi/show/base, srfi/159/* to VFS registry ([`d04061d`](https://github.com/emesal/tein/commit/d04061d4353c71df51005f8f59f1357938c14034))

- Add srfi/160 uniform vector library to VFS registry ([`55c84dd`](https://github.com/emesal/tein/commit/55c84ddaed1e36bdc3b3b70e062d0fd42f8b6879))

- Add scheme/vector/*, srfi/179, srfi/231, chibi/highlight to VFS ([`1942c81`](https://github.com/emesal/tein/commit/1942c81d2a787f183bf6e265a9eaa5c06b455d32))

- Add SHADOW_STUBS data + VfsEntry shells for OS-touching modules ([`e53ab75`](https://github.com/emesal/tein/commit/e53ab75ad1f56d16581859bfb11829187ad4dc99))

- Build.rs generates shadow stub .sld strings from SHADOW_STUBS ([`3194c16`](https://github.com/emesal/tein/commit/3194c16557f45290d2a26bc25daacd2b99534ffe))

- Register_vfs_shadows handles both hand-written and generated stubs ([`513c5ec`](https://github.com/emesal/tein/commit/513c5ecb0661e5c87238247f6bd5e6d10ed935a5))

- Add chibi/iset/optimize, chibi/show aliases, srfi/227/definition ([`2027329`](https://github.com/emesal/tein/commit/20273290288b9cf1ff13a855baea5b678a4cd387))

- Add chibi/mime, chibi/binary-record, chibi/memoize to VFS registry ([`5fd54e2`](https://github.com/emesal/tein/commit/5fd54e27f202d79c2d221816a65db08c124b15d4))

- Add chibi/stty, chibi/term/edit-line, chibi/app, chibi/config, chibi/log shadow stubs ([`a8e7853`](https://github.com/emesal/tein/commit/a8e78536e0f6740d6c5a3bc6c05d2035444221ec))

- Add chibi/tar, srfi/193, chibi/apropos shadow stubs + scheme/load hand-written shadow ([`d9a80f3`](https://github.com/emesal/tein/commit/d9a80f3efc21964734065afae5f4f06b135b513c))

- Add (chibi test) to VFS registry ([`220090a`](https://github.com/emesal/tein/commit/220090afbf6bdf23ffad65ab5d402ed362b07a3b))

- Add scheme/bytevector-test + srfi/N/test suites to VFS ([`a0ae862`](https://github.com/emesal/tein/commit/a0ae862ef57937f3d8a32697f1639429f5d20b95))

- Add chibi/X-test suites to VFS registry ([`b7bba15`](https://github.com/emesal/tein/commit/b7bba151f02547130b2a5ffea08dac511cfd50ef))

- Vfs_module_tests harness + shadow SLD fixes + srfi/chibi test integration ([`b670ed3`](https://github.com/emesal/tein/commit/b670ed380dac203c6dc4a98772b3d34206e24e75))

- Add timezone-offset-seconds trampoline to (tein time) ([`04093d9`](https://github.com/emesal/tein/commit/04093d9f72f9798421436e9d73d01aca69c269be))

- Wire (srfi 19) into VFS registry — resolve blocker ([`afd25ac`](https://github.com/emesal/tein/commit/afd25ac6353ff798c8af64b2ef6e4730ba79cf03))

- Add SANDBOX_ENV + SANDBOX_COMMAND_LINE thread-locals and builder methods (#99) ([`8210d6f`](https://github.com/emesal/tein/commit/8210d6f2b9823ed279c35f72d2083b6307746ecc))

- Wire SANDBOX_ENV + SANDBOX_COMMAND_LINE into build() and drop() (#99) ([`204d61a`](https://github.com/emesal/tein/commit/204d61ad368dad8d078b3e457c7b2b805569e902))

- Trampolines consult SANDBOX_ENV + SANDBOX_COMMAND_LINE (#99) ([`10f8a6e`](https://github.com/emesal/tein/commit/10f8a6e39d875d597bd3520bb5c94c7b98d2ddf8))

- **safe-regexp:** Scheme integration tests + suppress unused_assignment warning (#37) ([`38dc4ca`](https://github.com/emesal/tein/commit/38dc4ca7e630b64962d7db18b7b013a5f0536ae9))

- **value:** Add Value::Exit(i32) for (exit n) escape hatch ([`11e6a4d`](https://github.com/emesal/tein/commit/11e6a4dd8df20eabe36aae0a72de8f590f0b0206))

- **tein-bin:** Arg parsing — sandbox/all-modules/script/repl modes ([`2f02752`](https://github.com/emesal/tein/commit/2f027523c6b43ba89bc3a951d9001bb3ae35911b))

- **tein-bin:** Shebang stripping ([`232928d`](https://github.com/emesal/tein/commit/232928dbac398d2df65df77125e9adf0bd76d4fb))

- **tein-bin:** Script mode with sandbox flags ([`46a2f99`](https://github.com/emesal/tein/commit/46a2f99c7e6e5f72526df0492cc43004cfe80d4f))

- **tein-bin:** REPL mode with history, paren tracking, exit handling ([`7a9712c`](https://github.com/emesal/tein/commit/7a9712c5a1142ce1406cd2512640b3ec7f5768c5))

- **crypto:** Add sha2, blake3, rand deps + crypto feature gate (#38) ([`75fd07d`](https://github.com/emesal/tein/commit/75fd07d9338d2214de63ec6edc21a6b44791b8d2))

- **crypto:** Add (tein crypto) module with hashing + CSPRNG (#38) ([`ab90c30`](https://github.com/emesal/tein/commit/ab90c308e4e05459a249459ce61295a76b4e6e3b))

- **crypto:** Wire feature gates in build.rs + sandbox.rs (#38) ([`3512529`](https://github.com/emesal/tein/commit/3512529703474f6513a9afdf7b653a0937ffe912))

- **ffi:** Add sexp_set_parameter + port symbol getter bindings ([`c88b101`](https://github.com/emesal/tein/commit/c88b101d061262ebcd05fefbeaa15df760682df3))

- **ffi:** Meta env accessor + make-immutable bindings (#97) ([`0526280`](https://github.com/emesal/tein/commit/0526280262dbfb21a8051c6ba34ce413502241c5))

- **context:** INTERACTION_ENV thread-local + cleanup (#97) ([`55cf5c5`](https://github.com/emesal/tein/commit/55cf5c560923f6a3ad166b8b636bb44febe162ca))

- **sandbox:** Register eval trampolines in primitive env (#97) ([`2a33e8d`](https://github.com/emesal/tein/commit/2a33e8d42d23b007e2281e30232f5f5ece0dec23))

- **ffi:** Expose tein_vfs_lookup_static for collision detection (#132) ([`8c7f07f`](https://github.com/emesal/tein/commit/8c7f07f6f7f38267710baee845e070d4ffccbdff))

- **context:** Add CONTEXT_PTR thread-local and allow_module_runtime (#132) ([`f709556`](https://github.com/emesal/tein/commit/f709556374b9700f306ba50dbb71372704ad3aa0))

- **context:** Add register_module for dynamic module registration (#132) ([`3a25d8f`](https://github.com/emesal/tein/commit/3a25d8f1b54364dc6d28aae76ceeadf8038d25a2))

- **builder:** Add allow_dynamic_modules() convenience method (#132) ([`f513e82`](https://github.com/emesal/tein/commit/f513e827b2e5b5f67fb902f5e3d89dc0aa02c286))

- Add (tein modules) scheme API for dynamic module registration (#132) ([`d833752`](https://github.com/emesal/tein/commit/d8337523f8146fdb2be051cca4d7b2a16753707d))

- **http:** Add ureq dependency and feature gate (#130) ([`a859c85`](https://github.com/emesal/tein/commit/a859c8572596189b2350954b934648195b877ae2))

- **http:** Add http.rs with native request fn and tests (#130) ([`1f71189`](https://github.com/emesal/tein/commit/1f7118973ea9aef589bc4ed0e8191a64bc502e8e))

- **http:** Add VFS registry entry (#130) ([`021c9c8`](https://github.com/emesal/tein/commit/021c9c8c75df94cd21ee7db28686acd0542e2845))

- **http:** Add feature checks and dynamic module exports (#130) ([`9100883`](https://github.com/emesal/tein/commit/9100883ea9ae02d15b6d47ee1ed7af5852f0cb4a))

- **http:** Register trampoline and VFS modules in context (#130) ([`eae5561`](https://github.com/emesal/tein/commit/eae55615cd6c8dace37dab14e45171e1f2af7d0f))

- **ext:** Add make_error to TeinExtApi vtable, bump to v2 (#135) ([`033563a`](https://github.com/emesal/tein/commit/033563a0ad4864f7c106b41edea61036cf079ae8))

- **context:** Populate make_error in ext vtable (#135) ([`a4c1e08`](https://github.com/emesal/tein/commit/a4c1e082877ff7825be9c5ef7ed7cec2cc22abbd))

- **macros:** Result::Err raises exception in standalone/internal mode (#135) ([`34fad38`](https://github.com/emesal/tein/commit/34fad38c57ca2a966fc54b18b1164595eeb71820))

- **macros:** Result::Err raises exception in ext mode (#135) ([`cf76323`](https://github.com/emesal/tein/commit/cf76323cf0e760b7ca6d6a8bb0d76d8a956cc381))

- **trampolines:** Error paths raise exceptions instead of returning strings (#135) ([`91c7b29`](https://github.com/emesal/tein/commit/91c7b29a50c99448c7af7ba24e8adab96af5a737))

- **ffi:** Bind sexp_add_module_directory_op (#131) ([`0fe3f1e`](https://github.com/emesal/tein/commit/0fe3f1e672cd8abf44196dac7adf439d653bbcf8))

- **sandbox:** Add FS_MODULE_PATHS thread-local (#131) ([`800ea5b`](https://github.com/emesal/tein/commit/800ea5b4cb64845aa24cde3f3458fe4e34850180))

- **ffi:** Extend vfs gate check to allow fs module paths (#131) ([`6a6bbcc`](https://github.com/emesal/tein/commit/6a6bbcca09e5d77c44d7f46ce3dfb18d188938cc))

- **context:** Add ContextBuilder::module_path() (#131) ([`b4f8d42`](https://github.com/emesal/tein/commit/b4f8d4212194cc0a1e23bd19ca7ad730a5721c08))

- **tein-bin:** Add -I/--include-path flag (#131) ([`f57ed54`](https://github.com/emesal/tein/commit/f57ed54c4b65ad9a52e23c05353e20a87730a34d))

- **json:** Expose json_value_to_value as public API ([`7cb6955`](https://github.com/emesal/tein/commit/7cb6955e39ff92ebfeec80ed19413e73af9d949f))

- **sandbox:** Auto-import scheme/base + scheme/write in sandboxed contexts ([`f71da7a`](https://github.com/emesal/tein/commit/f71da7acf03d968d55bca6fcd8f5d6bf7c8ed7e7))

- Add (tein filesystem) module with real fs implementations ([`c018bd2`](https://github.com/emesal/tein/commit/c018bd22d6d995d33f314e8baf4c0d6132e5e63f))

- Add current-process-id and system to (tein process) ([`19ec4ca`](https://github.com/emesal/tein/commit/19ec4ca7f2f29afe4a7dbdeb489f7567e0237cac))


### Refactoring

- Extract shared thread protocol into thread.rs ([`1dcde09`](https://github.com/emesal/tein/commit/1dcde099f17a81dd6286689b154afb1081fb5543))

- Replace dispatch pattern with 3 individual native fns ([`aa48d9b`](https://github.com/emesal/tein/commit/aa48d9b08522d557022f558d16e4d3c8a15c65e6))

- VfsGate data structures + VFS module registry (steps 1–3) ([`501f75b`](https://github.com/emesal/tein/commit/501f75b80c20b1325c68fd6d817b3f6773aa7923))

- ModulePolicy → VfsGate rust-side migration (step 4) ([`b512add`](https://github.com/emesal/tein/commit/b512addc9d33aac68e050d90a3872284d87667e7))

- Remove VfsGate enum and vfs_gate_all/vfs_gate_none builder methods ([`8e73ca8`](https://github.com/emesal/tein/commit/8e73ca8b3b1d81c4119a267f9a72dce3f438ec04))

- Remove old IO wrapper system, policy enforcement via (tein file) trampolines ([`83c6ee5`](https://github.com/emesal/tein/commit/83c6ee5edd5e843b4b1af590a57d4c25bea55418))

- Remove open-*-file trampolines, simplify (scheme file) shadow ([`40a4709`](https://github.com/emesal/tein/commit/40a4709d5a4a428bee8498f443eaf9db3d5174da))

- **vfs:** Union deps across Embedded+Shadow entry pairs in registry_resolve_deps ([`1739e6b`](https://github.com/emesal/tein/commit/1739e6b84189d0118e2a4e8df328e15d41ba1984))

- (scheme time) shadow re-exports from (tein time) ([`c497906`](https://github.com/emesal/tein/commit/c4979063575a8c39cef59f7951599730cde557e5))

- **http:** Review fixes — shared ffi helpers, timeout clamp, TRACE support (#130) ([`af68481`](https://github.com/emesal/tein/commit/af684819bfa39d11763d03baf3aa3c3262efe644))

- Convert 9 shadow modules to embedded, fix deps ([`33673d1`](https://github.com/emesal/tein/commit/33673d1446371357e2ebe991d9209f81e6ae6fe6))

- Remove old file-exists?/delete-file trampolines ([`22b9650`](https://github.com/emesal/tein/commit/22b96508ad4777c395936488ba9d887d7e3f8cb1))


### Tests

- **serde:** Attribute compatibility tests (rename, default, flatten, tag) ([`5bf94e6`](https://github.com/emesal/tein/commit/5bf94e6b21fbd6707e9d96a1d49f2a78faee4b9e))

- **serde:** Edge case round-trip tests (floats, unicode, struct variants, map keys) ([`2e4a206`](https://github.com/emesal/tein/commit/2e4a206934707550005e83810801bd73cb55aa07))

- Foreign type registration, round-trip, dispatch, introspection, predicates ([`cfa3957`](https://github.com/emesal/tein/commit/cfa3957706ba0b7a93c1eda8aad82dd5587e6b47))

- Foreign sandbox integration and cleanup-on-drop ([`2712843`](https://github.com/emesal/tein/commit/27128436d89939a8f85aee749eb6ad94bd8141c3))

- Scheme control flow, binding forms, and TCO coverage ([`e00649d`](https://github.com/emesal/tein/commit/e00649dcabc962622eda95fe0ec3ed903ec7b988))

- Closures, continuations, and error handling coverage ([`73ab72b`](https://github.com/emesal/tein/commit/73ab72b3efcb9f80931a58f00693ac8f513148d4))

- Scheme test coverage tasks 7–16 (records through tein_foreign) ([`3af10e1`](https://github.com/emesal/tein/commit/3af10e16103c12ba253cda9ef104bc66621facd5))

- Doc preservation + integration tests for #60 ([`dbca8cc`](https://github.com/emesal/tein/commit/dbca8cc0d9a1e2305e96204ab5f083910cec1d9e))

- Integration tests for cdylib extension system (#62) ([`1f42f2e`](https://github.com/emesal/tein/commit/1f42f2e7f2d90ff6d3b6e1e346f88e9a1c544471))

- Scheme-level numeric tower integration tests (#71) ([`124ee01`](https://github.com/emesal/tein/commit/124ee01b52ec8752de5f14ad18b7dec248618b41))

- **json:** Add rust-level integration tests for (tein json) ([`568699f`](https://github.com/emesal/tein/commit/568699f20e17ef5c71ba714dbbb2f3816beacaeb))

- **json:** Add scheme-level integration tests for (tein json) ([`84de420`](https://github.com/emesal/tein/commit/84de420d182c5bf526d1fa299438bdf9e7e3fb5c))

- Update module policy tests for three-tier model (#86) ([`07eaad4`](https://github.com/emesal/tein/commit/07eaad4795a2f4f1d00ea9b7b051f1267fd6502d))

- Add tests for allowlist, vfs_all, allow_module, allow_only_modules (#86) ([`4e3ffe0`](https://github.com/emesal/tein/commit/4e3ffe0896ffa9d94e4abeb9ecbd56100c96e8fd))

- Add scheme-level integration tests for (tein file/process) (#87) ([`8893cf4`](https://github.com/emesal/tein/commit/8893cf4ea24b8e689f07876a42c9330b1c37d7ac))

- Rust integration tests for (tein uuid) ([`9550765`](https://github.com/emesal/tein/commit/9550765091c0823431b29765d57a8d70a9b8ae25))

- Scheme-level (tein uuid) integration tests ([`51ab561`](https://github.com/emesal/tein/commit/51ab561b9b28793867e9b47f84e09d206fb58718))

- **time:** Add rust integration tests for (tein time) (#90) ([`162a6e7`](https://github.com/emesal/tein/commit/162a6e7553475094bfe24581f89ad7d34d9ef97f))

- **time:** Add scheme-level tests for (tein time) (#90) ([`7d89969`](https://github.com/emesal/tein/commit/7d8996917f0e54fe6b5c57c58456554204a80c5b))

- VFS shadow integration tests for (scheme file) + (scheme repl) ([`8383585`](https://github.com/emesal/tein/commit/8383585a7581c98445d96225b7f44bd14ad44953))

- Add srfi/166/columnar from-file integration tests (blocked until C-level FsPolicy) ([`2ca2654`](https://github.com/emesal/tein/commit/2ca26542f415a93624537d7603672ec13d6456d8))

- Add test_fs_gate_cleared_on_drop (mirrors test_vfs_gate_cleared_on_drop) ([`e1e86b6`](https://github.com/emesal/tein/commit/e1e86b64eb7b352558b0db67cf4c2399b089ebd0))

- Add binary file open denied tests in sandbox ([`a9a6640`](https://github.com/emesal/tein/commit/a9a6640221d087bb2596a7e6d729b69d0ad963d4))

- Shadow stub integration tests + chibi/channel VFS test ([`f3a1f8f`](https://github.com/emesal/tein/commit/f3a1f8fefc44af22c4ae241688f6a6cf955981a2))

- Chibi/iset/optimize, chibi/show aliases, srfi/227/definition smoke tests ([`b316380`](https://github.com/emesal/tein/commit/b316380b62ce6aecf67323c0b2448774bc7d43e6))

- Verify shadow stubs absent from unsandboxed context ([`8975ff6`](https://github.com/emesal/tein/commit/8975ff652a9489cce7dd957413c0bbe89364d359))

- Srfi/144 flonum constants, scheme/bytevector endian, chibi/time import ([`f1a1089`](https://github.com/emesal/tein/commit/f1a1089d773517fe8c1c87d9549de7564d4a10c1))

- Scheme/char, scheme/division, scheme/fixnum, scheme/bitwise coverage ([`916e8eb`](https://github.com/emesal/tein/commit/916e8eb516652b261548d56a5af3b4a794c7fe9a))

- Scheme/flonum constants + transcendentals, srfi/18 thread/mutex/condvar ([`6221ce2`](https://github.com/emesal/tein/commit/6221ce2aea7d7a2534ccb3cf9293ef43bae6f9a7))

- Sandbox fake env vars + command-line; update VFS shadows to delegate to trampolines (#99) ([`95b0d5f`](https://github.com/emesal/tein/commit/95b0d5f354d4ec1c0e93124c01ef8f3a4e930aec))

- **crypto:** Hash + CSPRNG + sandbox tests with NIST/reference vectors (#38) ([`779c461`](https://github.com/emesal/tein/commit/779c461fae440f16c2dce673601cbbd494646fc6))

- **crypto:** Scheme integration tests for (tein crypto) (#38) ([`eaae04c`](https://github.com/emesal/tein/commit/eaae04cddcfd60e4550122644cc7e82ce9066b29))

- Add (chibi regexp) SRFI-115 smoke tests (#85) ([`077076f`](https://github.com/emesal/tein/commit/077076fe440db5863e2223f93800194a251c2535))

- Sandbox gating tests for (chibi regexp) and aliases (#85) ([`1fb13a5`](https://github.com/emesal/tein/commit/1fb13a514d329b17ec78fa31a6550dec1d92eaa4))

- **sandbox:** Verify dangerous chibi primitives are blocked; clarify shadow SLD rules ([`c3c90a0`](https://github.com/emesal/tein/commit/c3c90a0d8ecb7f848c511e44deec72c40ca05a81))

- **scheme:** Integration tests for dynamic module registration (#132) ([`a07ad5a`](https://github.com/emesal/tein/commit/a07ad5a174613e0394e92e0e9e403211515626bb))

- **http:** Integration tests for (tein http) module (#130) ([`4134cfc`](https://github.com/emesal/tein/commit/4134cfcd810553a5a5330b23cdfb0c283016a15a))

- Update #[tein_fn] tests for exception error returns (#135) ([`da7ac67`](https://github.com/emesal/tein/commit/da7ac67699b1cb75428e6983a84e75c893ed8f94))

- Update json/toml/http tests for exception error returns (#135) ([`c74d7fd`](https://github.com/emesal/tein/commit/c74d7fdf635be6fddf8f9b8b4f2ccfdaa64a3144))

- Update ext and scheme tests for exception error returns (#135) ([`fea84f2`](https://github.com/emesal/tein/commit/fea84f238f48082a35f5671c28ebd854eed5de8c))

- **context:** Integration tests for module_path (#131) ([`8869377`](https://github.com/emesal/tein/commit/88693773e9c53e7e59e08be8f1aba2018a529099))

- Add exit dynamic-wind and emergency-exit tests (#101) ([`dd82b89`](https://github.com/emesal/tein/commit/dd82b89f9c73bd17630d2e1b0034b1b7bc937d7c))

- **sandbox:** Add tests for auto-import behaviour ([`4c00754`](https://github.com/emesal/tein/commit/4c00754766c9c0afd5844c8c547cf6c35803963d))

- Unsandboxed module availability + chibi/diff integration ([`d660b80`](https://github.com/emesal/tein/commit/d660b8093598db832f39ef7d040c2c19a9c5f4ec))


