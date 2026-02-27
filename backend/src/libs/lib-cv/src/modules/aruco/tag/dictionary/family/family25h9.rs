use crate::modules::aruco::tag::{ArucoTagDecode, ArucoTagDecoding, decode};

pub struct Family25H9;

impl Family25H9 {
    pub const NAME: &'static str = "25h9";
}

impl ArucoTagDecoding for Family25H9 {
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

    /// Returns the size of the data grid.
    fn total_width(&self) -> u8 {
        Self::TOTAL_WIDTH
    }

    fn codes(&self) -> &[u64] {
        &Self::CODES
    }
}
impl Family25H9 {
    // "h9" is the *minimum* Hamming distance between codes; the maximum number of correctable
    // bit errors is floor((d-1)/2).
    const HAMMING: u8 = 4;
    const BORDER_SIZE: u8 = 1;
    const TOTAL_WIDTH: u8 = 9;
    const DATA_WIDTH: u8 = 7;
    const DATA_BITS_LOCATION: [(u8, u8); 25] = [
        (0, 0),
        (1, 0),
        (2, 0),
        (3, 0),
        (1, 1),
        (2, 1),
        (4, 0),
        (4, 1),
        (4, 2),
        (4, 3),
        (3, 1),
        (3, 2),
        (4, 4),
        (3, 4),
        (2, 4),
        (1, 4),
        (3, 3),
        (2, 3),
        (0, 4),
        (0, 3),
        (0, 2),
        (0, 1),
        (1, 3),
        (1, 2),
        (2, 2),
    ];
    const CODES: [u64; 35] = [
        0x000000000156f1f4,
        0x0000000001f28cd5,
        0x00000000016ce32c,
        0x0000000001ea379c,
        0x0000000001390f89,
        0x000000000034fad0,
        0x00000000007dcdb5,
        0x000000000119ba95,
        0x0000000001ae9daa,
        0x0000000000df02aa,
        0x000000000082fc15,
        0x0000000000465123,
        0x0000000000ceee98,
        0x0000000001f17260,
        0x00000000014429cd,
        0x00000000017248a8,
        0x00000000016ad452,
        0x00000000009670ad,
        0x00000000016f65b2,
        0x0000000000b8322b,
        0x00000000005d715b,
        0x0000000001a1c7e7,
        0x0000000000d7890d,
        0x0000000001813522,
        0x0000000001c9c611,
        0x000000000099e4a4,
        0x0000000000855234,
        0x00000000017b81c0,
        0x0000000000c294bb,
        0x000000000089fae3,
        0x000000000044df5f,
        0x0000000001360159,
        0x0000000000ec31e8,
        0x0000000001bcc0f6,
        0x0000000000a64f8d,
    ];
}
