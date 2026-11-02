#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cbp_fuzz::fuzz_frame_reader(data);
    cbp_fuzz::fuzz_stream_resync(data);
});
