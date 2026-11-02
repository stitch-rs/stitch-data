#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cbp_fuzz::fuzz_unpack_frame(data);
    cbp_fuzz::fuzz_derived_record(data);
});
