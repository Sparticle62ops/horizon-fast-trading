// Track the override key (Left Alt) so a blocked trade can be pressed anyway.
fn fast_trading_keys(key: &str, action: mod_api::KeyAction) {
    crate::game::fast_trading::key(key, action)
}
