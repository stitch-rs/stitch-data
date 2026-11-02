#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cbp_fuzz::fuzz_bitpack(data);
});
