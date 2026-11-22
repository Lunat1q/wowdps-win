//! wowdps's app icon, drawn and packed as a Windows resource for the
//! binaries' build scripts to link: an icon group of id 1 — what Explorer
//! shows for the exe, what GPUI loads for its windows and the tray loads
//! for itself — over images from 16 to 256 px. Three rising gold bars on a
//! dark rounded tile. A build script includes this file by path (`#[path]`):
//! build scripts here take no crate.

use std::path::Path;

/// The image sides, each its own `RT_ICON`.
pub const SIDES: [u32; 6] = [16, 24, 32, 48, 64, 256];

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const EN_US: u16 = 0x0409;
/// MOVEABLE | PURE | DISCARDABLE, as rc.exe writes icons.
const ICON_FLAGS: u16 = 0x1030;

/// Write the resource into `OUT_DIR` and have cargo link it into the
/// package's binaries when building for Windows with MSVC (its linker
/// takes a `.res` as is); elsewhere nothing.
pub fn link() {
    println!("cargo:rerun-if-changed=../build-support/icon.rs");
    let target = |key: &str, want: &str| std::env::var(key).is_ok_and(|v| v == want);
    if !target("CARGO_CFG_TARGET_OS", "windows") || !target("CARGO_CFG_TARGET_ENV", "msvc") {
        return;
    }
    let Some(out) = std::env::var_os("OUT_DIR") else {
        return;
    };
    let path = Path::new(&out).join("wowdps-icon.res");
    if std::fs::write(&path, res()).is_ok() {
        println!("cargo:rustc-link-arg-bins={}", path.display());
    }
}

/// The whole `.res`: the empty entry every one opens with, each image,
/// then the group naming them.
pub fn res() -> Vec<u8> {
    let mut out = Vec::new();
    entry(&mut out, 0, 0, 0, 0, &[]);
    let mut group = Vec::new();
    for v in [0u16, 1, SIDES.len() as u16] {
        group.extend_from_slice(&v.to_le_bytes());
    }
    for (i, &side) in SIDES.iter().enumerate() {
        let image = dib(side);
        let id = i as u16 + 1;
        entry(&mut out, RT_ICON, id, ICON_FLAGS, EN_US, &image);
        // A side of 256 is written 0: the field is a byte.
        let byte = (side % 256) as u8;
        group.extend_from_slice(&[byte, byte, 0, 0]);
        group.extend_from_slice(&1u16.to_le_bytes());
        group.extend_from_slice(&32u16.to_le_bytes());
        group.extend_from_slice(&(image.len() as u32).to_le_bytes());
        group.extend_from_slice(&id.to_le_bytes());
    }
    entry(&mut out, RT_GROUP_ICON, 1, ICON_FLAGS, EN_US, &group);
    out
}

/// One resource: its 32-byte header (type and name by number), its data,
/// padded to four bytes.
fn entry(out: &mut Vec<u8>, kind: u16, name: u16, flags: u16, lang: u16, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    for v in [0xffff, kind, 0xffff, name] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&lang.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(data);
    out.resize(out.len().next_multiple_of(4), 0);
}

/// One image as an icon stores it: a 32-bit DIB of twice its height, BGRA
/// rows bottom-up, then an all-clear AND mask (the alpha draws the edge).
fn dib(side: u32) -> Vec<u8> {
    let mask_row = side.div_ceil(32) * 4;
    let mut out = Vec::new();
    for v in [40, side, side * 2] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(side * side * 4 + mask_row * side).to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    let rows: Vec<Vec<u8>> = (0..side).map(|y| row(side, y)).collect();
    for r in rows.iter().rev() {
        out.extend_from_slice(r);
    }
    out.resize(out.len() + (mask_row * side) as usize, 0);
    out
}

/// Row `y` (from the top) of the icon at `side` px, BGRA, straight alpha:
/// each pixel the share of its 4×4 samples on the tile and on a bar.
fn row(side: u32, y: u32) -> Vec<u8> {
    const TILE: [f32; 3] = [0x30 as f32, 0x22 as f32, 0x1e as f32];
    const BAR: [f32; 3] = [0x4a as f32, 0xb8 as f32, 0xe8 as f32];
    const N: u32 = 4;
    let mut out = Vec::new();
    for x in 0..side {
        let (mut on, mut bar) = (0u32, 0u32);
        for sy in 0..N {
            for sx in 0..N {
                let u = (x as f32 + (sx as f32 + 0.5) / N as f32) / side as f32;
                let v = (y as f32 + (sy as f32 + 0.5) / N as f32) / side as f32;
                match sample(u, v) {
                    Some(true) => (on, bar) = (on + 1, bar + 1),
                    Some(false) => on += 1,
                    None => {}
                }
            }
        }
        let share = if on == 0 { 0.0 } else { bar as f32 / on as f32 };
        for (t, b) in TILE.iter().zip(BAR) {
            out.push((t + (b - t) * share).round() as u8);
        }
        out.push((on * 255 / (N * N)) as u8);
    }
    out
}

/// The design at `(u, v)` in the unit square: `None` off the tile, else
/// whether a bar is there.
fn sample(u: f32, v: f32) -> Option<bool> {
    const RADIUS: f32 = 0.19;
    // Each bar's left and right edges and its top; all stand on 0.81.
    const BARS: [(f32, f32, f32); 3] =
        [(0.22, 0.34, 0.53), (0.44, 0.56, 0.375), (0.66, 0.78, 0.22)];
    let (du, dv) = (
        u - u.clamp(RADIUS, 1.0 - RADIUS),
        v - v.clamp(RADIUS, 1.0 - RADIUS),
    );
    if du * du + dv * dv > RADIUS * RADIUS {
        return None;
    }
    Some(
        BARS.iter()
            .any(|&(l, r, top)| (l..r).contains(&u) && (top..0.81).contains(&v)),
    )
}
