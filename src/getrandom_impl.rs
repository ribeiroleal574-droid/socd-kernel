// Implementação de getrandom para no_std bare-metal
// Necessária para ed25519-dalek funcionar sem OS
use getrandom::{register_custom_getrandom, Error};

fn socd_getrandom(buf: &mut [u8]) -> Result<(), Error> {
    crate::crypto::random_bytes(buf);
    Ok(())
}

register_custom_getrandom!(socd_getrandom);
