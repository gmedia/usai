#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    usai_runtime::fuzz_surface::postgres_params(data);
});
