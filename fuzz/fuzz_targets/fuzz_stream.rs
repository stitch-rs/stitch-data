#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    stitch_data_fuzz::fuzz_frame_reader(data);
    stitch_data_fuzz::fuzz_stream_resync(data);
});
