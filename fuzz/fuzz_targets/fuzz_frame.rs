#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    stitch_data_fuzz::fuzz_unpack_frame(data);
    stitch_data_fuzz::fuzz_derived_record(data);
});
