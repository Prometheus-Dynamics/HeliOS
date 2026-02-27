use crate::modules::aruco::tag::{ArucoTagDecode, ArucoTagDecoding, decode};

pub struct Family16H5;

impl Family16H5 {
    pub const NAME: &'static str = "16h5";
}

impl ArucoTagDecoding for Family16H5 {
    fn decode(&self, code: Vec<Vec<u8>>) -> Option<ArucoTagDecode> {
        decode(&code, self)
    }

    /// Returns the maximum allowable Hamming distance for decoding.
    fn max_hamming_distance(&self) -> u8 {
        Self::HAMMING
    }

    /// Returns the locations of data bits in the grid.
    fn data_bits_location(&self) -> &[(u8, u8)] {
        &Self::DATA_BITS_LOCATION
    }

    /// Returns the size of the border around the tag.
    fn border_size(&self) -> u8 {
        Self::BORDER_SIZE
    }

    /// Returns the size of the data grid.
    fn data_width(&self) -> u8 {
        Self::DATA_WIDTH
    }

    fn total_width(&self) -> u8 {
        Self::TOTAL_WIDTH
    }

    fn codes(&self) -> &[u64] {
        &Self::CODES
    }
}

impl Family16H5 {
    // "h5" is the *minimum* Hamming distance between codes; the maximum number of correctable
    // bit errors is floor((d-1)/2).
    const HAMMING: u8 = 2;
    const BORDER_SIZE: u8 = 1;
    const TOTAL_WIDTH: u8 = 6;
    const DATA_WIDTH: u8 = 4;
    const DATA_BITS_LOCATION: [(u8, u8); 16] = [(0, 0), (1, 0), (2, 0), (1, 1), (3, 0), (3, 1), (3, 2), (2, 1), (3, 3), (2, 3), (1, 3), (2, 2), (0, 3), (0, 2), (0, 1), (1, 2)];

    const CODES: [u64; 30] = [
        0x00000000000027c8u64,
        0x00000000000031b6u64,
        0x0000000000003859u64,
        0x000000000000569cu64,
        0x0000000000006c76u64,
        0x0000000000007ddbu64,
        0x000000000000af09u64,
        0x000000000000f5a1u64,
        0x000000000000fb8bu64,
        0x0000000000001cb9u64,
        0x00000000000028cau64,
        0x000000000000e8dcu64,
        0x0000000000001426u64,
        0x0000000000005770u64,
        0x0000000000009253u64,
        0x000000000000b702u64,
        0x000000000000063au64,
        0x0000000000008f34u64,
        0x000000000000b4c0u64,
        0x00000000000051ecu64,
        0x000000000000e6f0u64,
        0x0000000000005fa4u64,
        0x000000000000dd43u64,
        0x0000000000001aaau64,
        0x000000000000e62fu64,
        0x0000000000006dbcu64,
        0x000000000000b6ebu64,
        0x000000000000de10u64,
        0x000000000000154du64,
        0x000000000000b57au64,
    ];
}
