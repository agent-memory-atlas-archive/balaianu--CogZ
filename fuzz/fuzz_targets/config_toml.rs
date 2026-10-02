#![no_main]

use libfuzzer_sys::fuzz_target;

// config.toml is user-edited and deserialized+validated at every
// startup. Serde rejects malformed input; validate() must also
// reject (not panic on) semantically wrong values.
fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(config) = toml::from_str::<cogz::config::Config>(text) {
        let _ = config.validate();
    }
});
