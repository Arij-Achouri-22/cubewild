//! Versioned block storage. Numeric IDs are permanent; new families use new ranges.
//! Legacy byte chunks are read by World and rewritten only by a successful checkpoint.
use crate::world::{Block, ChunkData, VOL};
use std::io::{self, ErrorKind};

pub const MAGIC: &[u8; 4] = b"CWB2";
pub const VERSION: u16 = 1;
pub const LIMIT: usize = 1024;
/// Inclusive ranges of permanent IDs. Extend this registry when adding a family;
/// existing IDs and state layouts must keep their meaning.
pub const FAMILIES: &[(Block, Block, &str)] = &[
    (0, 255, "legacy blocks and states"),
    (256, 257, "equipment stations"),
    (258, 264, "moist farmland"),
    (272, 279, "carrots and potatoes"),
    (288, 295, "fence gates"),
    (304, 307, "signs"),
    (320, 327, "additional slabs"),
    (352, 383, "additional stairs"),
];
pub fn valid(block: Block) -> bool {
    FAMILIES.iter().any(|&(first, last, _)| (first..=last).contains(&block))
}

pub fn encode(data: &ChunkData) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(20 + VOL * 2);
    bytes.extend(MAGIC);
    bytes.extend(VERSION.to_le_bytes());
    bytes.extend(1u16.to_le_bytes()); // registry version
    bytes.extend((VOL as u32).to_le_bytes());
    bytes.extend(0u64.to_le_bytes());
    for &block in &data.blocks {
        bytes.extend(block.to_le_bytes());
    }
    let hash = checksum(&bytes[20..]);
    bytes[12..20].copy_from_slice(&hash.to_le_bytes());
    bytes
}

pub fn decode(bytes: &[u8]) -> io::Result<ChunkData> {
    let bad = || io::Error::new(ErrorKind::InvalidData, "unsupported or damaged versioned chunk");
    if bytes.len() != 20 + VOL * 2
        || &bytes[..4] != MAGIC
        || u16::from_le_bytes(bytes[4..6].try_into().unwrap()) != VERSION
        || u16::from_le_bytes(bytes[6..8].try_into().unwrap()) != 1
        || u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize != VOL
        || u64::from_le_bytes(bytes[12..20].try_into().unwrap()) != checksum(&bytes[20..])
    {
        return Err(bad());
    }
    let blocks: Box<[Block]> = bytes[20..].chunks_exact(2).map(|b| Block::from_le_bytes([b[0], b[1]])).collect();
    if blocks.iter().any(|&b| !valid(b)) {
        return Err(bad());
    }
    Ok(ChunkData { blocks })
}

fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extended_ids_round_trip_and_corruption_or_unknown_versions_fail() {
        let data = ChunkData { blocks: vec![352; VOL].into_boxed_slice() };
        let encoded = encode(&data);
        assert_eq!(decode(&encoded).unwrap().blocks, data.blocks);
        for end in [0, 4, 19, encoded.len() - 1] {
            assert!(decode(&encoded[..end]).is_err());
        }
        let mut changed = encoded.clone();
        changed[24] ^= 1;
        assert!(decode(&changed).is_err());
        assert!(decode(&encode(&ChunkData { blocks: vec![512; VOL].into_boxed_slice() })).is_err());
        let mut changed = encoded;
        changed[4] = 99;
        assert!(decode(&changed).is_err());
    }
}
