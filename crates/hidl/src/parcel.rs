//! LP64 libhwbinder Parcel: four-byte inline padding; eight-byte SG buffer padding.
pub const PTR: u32 = 0x7074_2a85;
pub const BINDER: u32 = 0x7362_2a85;
#[derive(Default, Debug)]
pub struct Parcel {
    pub data: Vec<u8>,
    pub offsets: Vec<u64>,
    pub buffers: Vec<Box<[u8]>>,
}
impl Parcel {
    pub fn scalar(&mut self, value: i32) {
        self.data.extend(value.to_le_bytes());
    }
    pub fn boolean(&mut self, value: bool) {
        self.scalar(i32::from(value));
    }
    pub fn token(&mut self, value: &str) {
        self.data.extend(value.as_bytes());
        self.data.push(0);
        self.data.resize(self.data.len().next_multiple_of(4), 0);
    }
    pub fn buffer(&mut self, bytes: Vec<u8>, parent: Option<(usize, usize)>) -> usize {
        let index = self.offsets.len();
        let bytes = bytes.into_boxed_slice();
        self.offsets.push(self.data.len() as u64);
        self.data.extend(PTR.to_le_bytes());
        self.data.extend(u32::from(parent.is_some()).to_le_bytes());
        self.data.extend(
            (if bytes.is_empty() {
                0
            } else {
                bytes.as_ptr() as u64
            })
            .to_le_bytes(),
        );
        self.data.extend((bytes.len() as u64).to_le_bytes());
        let (p, offset) = parent.unwrap_or_default();
        self.data.extend((p as u64).to_le_bytes());
        self.data.extend((offset as u64).to_le_bytes());
        self.buffers.push(bytes);
        index
    }
    pub fn string(&mut self, text: &str) {
        let mut header = vec![0; 16];
        header[8..12].copy_from_slice(&(text.len() as u32).to_le_bytes());
        let parent = self.buffer(header, None);
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        self.buffer(bytes, Some((parent, 0)));
    }
    pub fn strings(&mut self, values: &[&str]) {
        let mut header = vec![0; 16];
        header[8..12].copy_from_slice(&(values.len() as u32).to_le_bytes());
        let parent = self.buffer(header, None);
        let mut strings = vec![0; values.len() * 16];
        for (i, s) in values.iter().enumerate() {
            strings[i * 16 + 8..i * 16 + 12].copy_from_slice(&(s.len() as u32).to_le_bytes());
        }
        let child = self.buffer(strings, Some((parent, 0)));
        for (i, s) in values.iter().enumerate() {
            let mut bytes = s.as_bytes().to_vec();
            bytes.push(0);
            self.buffer(bytes, Some((child, i * 16)));
        }
    }
    pub fn command(&mut self, result: Result<(), i32>) {
        let text = match result {
            Ok(()) => "",
            Err(-1) => "Invalid slot",
            Err(_) => "Command failed",
        };
        let mut header = vec![0; 24];
        header[0] = u8::from(result.is_ok());
        header[16..20].copy_from_slice(&(text.len() as u32).to_le_bytes());
        let parent = self.buffer(header, None);
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        self.buffer(bytes, Some((parent, 8)));
    }
    pub fn binder(&mut self) {
        self.offsets.push(self.data.len() as u64);
        self.data.extend(BINDER.to_le_bytes());
        // AOSP BHwBinder defaults SCHED_NORMAL, priority 0; accept FDs and inherit RT.
        self.data.extend(0x900u32.to_le_bytes());
        self.data.extend(1u64.to_le_bytes());
        self.data.extend(1u64.to_le_bytes());
    }
    pub fn buffers_size(&self) -> u64 {
        self.buffers
            .iter()
            .map(|b| b.len().next_multiple_of(8) as u64)
            .sum()
    }
}
pub fn input(data: &[u8], token: &str, argument: bool) -> Result<i32, i32> {
    let end = data.iter().position(|v| *v == 0).ok_or(-22)?;
    if &data[..end] != token.as_bytes() {
        return Err(-1);
    }
    let start = (end + 1).next_multiple_of(4);
    if argument {
        Ok(i32::from_le_bytes(
            data.get(start..start + 4)
                .ok_or(-22)?
                .try_into()
                .map_err(|_| -22)?,
        ))
    } else {
        Ok(0)
    }
}
