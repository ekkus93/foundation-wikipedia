# Rust selection-test formatting failure — resolved

Rust CI on `668b5cb1026b1014a6d4f29cdd24e6708e5b4b17` failed in `crates/wiki-pack-builder/tests/selection_contract.rs` because rustfmt expected a one-line assertion. The test coverage was retained and reorganized into formatted selection contract, limits and regression test modules.

The correction passed Rust workspace CI `37960353505` on exact SHA `5a93e4ab00a3de99bab3d3c7ee27381fe2d9aa44`. Additional regression tests and portable manifest hardening passed Rust CI `37961256845` on exact SHA `5388d7aeade12ac7177ce76642b4000897ef1274`. Platform shell qualification is tracked separately and does not follow automatically from a Rust pass.
