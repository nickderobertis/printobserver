//! Prove the installed Rust client against a real running supervisor.
//!
//! This is built against the **packaged crate** rather than against this
//! repository's sources: the manifest beside it is written by the installer and
//! points at the unpacked package, so what it links is what a dependent would
//! get from the registry. A smoke check that reached no server would say
//! nothing about the artifact, so it makes two real calls — a status read, and
//! an image materialization whose answered path it opens and whose bytes it
//! checks against the digest the image record itself declares.
//!
//! ```console
//! printobserver-sdk-smoke --server http://127.0.0.1:8420 --print-id <id> --image-id <id>
//! ```

use std::process::ExitCode;

use printobserver_sdk::{Actor, CONTRACT_VERSION, Client};

/// One named argument, or `None` when it was not given.
fn argument(name: &str) -> Option<String> {
    let arguments: Vec<String> = std::env::args().collect();
    let at = arguments.iter().position(|given| given == &format!("--{name}"))?;
    arguments.get(at + 1).cloned()
}

/// The digest of one file's bytes, in lowercase hexadecimal.
fn digest_of(path: &std::path::Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{} could not be read: {error}", path.display()))?;
    Ok(sha256(&bytes))
}

/// SHA-256, written out rather than taken as a dependency: what this proves is
/// the client, and a smoke check with a dependency tree of its own would prove
/// that tree resolves as well.
fn sha256(message: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a, 0x510e_527f, 0x9b05_688c, 0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let mut padded = message.to_vec();
    let bits = (message.len() as u64) * 8;
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bits.to_be_bytes());

    for block in padded.chunks_exact(64) {
        let mut words = [0_u32; 64];
        for (index, chunk) in block.chunks_exact(4).enumerate() {
            words[index] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for index in 16..64 {
            let one = words[index - 15];
            let two = words[index - 2];
            let s0 = one.rotate_right(7) ^ one.rotate_right(18) ^ (one >> 3);
            let s1 = two.rotate_right(17) ^ two.rotate_right(19) ^ (two >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }
        let mut working = state;
        for index in 0..64 {
            let s1 = working[4].rotate_right(6) ^ working[4].rotate_right(11) ^ working[4].rotate_right(25);
            let choice = (working[4] & working[5]) ^ ((!working[4]) & working[6]);
            let first = working[7]
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let s0 = working[0].rotate_right(2) ^ working[0].rotate_right(13) ^ working[0].rotate_right(22);
            let majority = (working[0] & working[1]) ^ (working[0] & working[2]) ^ (working[1] & working[2]);
            let second = s0.wrapping_add(majority);
            working = [
                first.wrapping_add(second),
                working[0],
                working[1],
                working[2],
                working[3].wrapping_add(first),
                working[4],
                working[5],
                working[6],
            ];
        }
        for (index, value) in working.iter().enumerate() {
            state[index] = state[index].wrapping_add(*value);
        }
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}

/// Make the two calls, answering a process exit status.
fn main() -> ExitCode {
    let Some(server) = argument("server") else {
        eprintln!("smoke: --server takes an address and was given none");
        return ExitCode::from(2);
    };
    let (Some(print_id), Some(image_id)) = (argument("print-id"), argument("image-id")) else {
        eprintln!("smoke: --print-id and --image-id each take an identifier");
        return ExitCode::from(2);
    };
    let client = Client::new(server, Actor::Operator);

    let status = match client.status(&print_id) {
        Ok(status) => status,
        Err(failed) => {
            eprintln!("smoke: the status read failed: {failed}");
            return ExitCode::FAILURE;
        }
    };
    if status.print.id != print_id {
        eprintln!("smoke: the status read answered another print: {}", status.print.id);
        return ExitCode::FAILURE;
    }

    let answered = match client.image(&image_id) {
        Ok(answered) => answered,
        Err(failed) => {
            eprintln!("smoke: the image read failed: {failed}");
            return ExitCode::FAILURE;
        }
    };
    let Some(path) = answered.path.as_ref() else {
        eprintln!("smoke: the image read answered no path on the server's own host");
        return ExitCode::FAILURE;
    };
    let path = std::path::Path::new(path);
    if !path.is_absolute() {
        eprintln!("smoke: the image read answered {}, which is not absolute", path.display());
        return ExitCode::FAILURE;
    }
    let digest = match digest_of(path) {
        Ok(digest) => digest,
        Err(failed) => {
            eprintln!("smoke: {failed}");
            return ExitCode::FAILURE;
        }
    };
    if digest != answered.record.sha256 {
        eprintln!(
            "smoke: {} is not the image the record declares ({digest} vs {})",
            path.display(),
            answered.record.sha256
        );
        return ExitCode::FAILURE;
    }

    println!(
        "printobserver-sdk smoke: contract {CONTRACT_VERSION}, print {:?}, image {} at {}",
        status.print.state,
        &answered.record.sha256[..12],
        path.display()
    );
    ExitCode::SUCCESS
}
