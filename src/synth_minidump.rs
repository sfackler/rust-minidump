// Copyright 2016 Ted Mielczarek. See the COPYRIGHT
// file at the top-level directory of this distribution.

use encoding::all::UTF_16LE;
use encoding::{Encoding, EncoderTrap};
use minidump_format as md;
use std::marker::PhantomData;
use std::mem;
use test_assembler::*;

/// A writer of synthetic minidumps.
pub struct SynthMinidump {
    /// The `Section` containing the minidump contents.
    section: Section,
    /// The minidump flags, for the header.
    flags: Label,
    /// The number of streams.
    stream_count: u32,
    /// The number of streams, as a label for the header.
    stream_count_label: Label,
    /// The directory's file offset, for the header.
    stream_directory_rva: Label,
    /// The contents of the stream directory.
    stream_directory: Section,
}

/// A block of data contained in a minidump.
pub trait DumpSection : Into<Section> {
    /// Append an `MDLocationDescriptor` to `section` referring to this section.
    fn cite_location_in(&self, section: Section) -> Section {
        // An MDLocationDescriptor is just a 32-bit size + 32-bit offset.
        section
            .D32(&self.file_size())
            .D32(&self.file_offset())
    }
    /// A label representing this `DumpSection`'s offset in bytes from the start of the minidump.
    fn file_offset(&self) -> Label;
    /// A label representing this `DumpSection`'s size in bytes within the minidump.
    fn file_size(&self) -> Label;
}

/// A minidump stream.
pub trait Stream : DumpSection {
    /// The stream type, used in the stream directory.
    fn stream_type(&self) -> u32;
    /// Append an `MDRawDirectory` referring to this stream to `section`.
    fn cite_stream_in(&self, section: Section) -> Section {
        let section = section.D32(self.stream_type());
        self.cite_location_in(section)
    }
}

impl SynthMinidump {
    /// Create a `SynthMinidump` with default endianness.
    pub fn new() -> SynthMinidump {
        SynthMinidump::with_endian(DEFAULT_ENDIAN)
    }

    /// Create a `SynthMinidump` with `endian` endianness.
    pub fn with_endian(endian: Endian) -> SynthMinidump {
        let flags = Label::new();
        let stream_count_label = Label::new();
        let stream_directory_rva = Label::new();
        let section = Section::with_endian(endian)
            .D32(md::MD_HEADER_SIGNATURE)
            .D32(md::MD_HEADER_VERSION)
            .D32(&stream_count_label)
            .D32(&stream_directory_rva)
            .D32(0)
            .D32(1262805309) // date_time_stamp, arbitrary
            .D64(&flags);
        section.start().set_const(0);
        assert_eq!(section.size(), mem::size_of::<md::MDRawHeader>() as u64);

        SynthMinidump {
            section: section,
            flags: flags,
            stream_count: 0,
            stream_count_label: stream_count_label,
            stream_directory_rva: stream_directory_rva,
            stream_directory: Section::with_endian(endian),
        }
    }

    /// Set the minidump flags to `flags`.
    pub fn flags(self, flags: u64) -> SynthMinidump {
        self.flags.set_const(flags);
        self
    }

    /// Append `section` to `self`, setting its location appropriately.
    pub fn add<T: DumpSection>(mut self, section: T) -> SynthMinidump {
        let offset = section.file_offset();
        self.section = self.section
            .mark(&offset)
            .append_section(section);
        self
    }

    /// Append `stream` to `self`, setting its location appropriately and adding it to the stream directory.
    pub fn add_stream<T: Stream>(mut self, stream: T) -> SynthMinidump {
        self.stream_directory = stream.cite_stream_in(self.stream_directory);
        self.stream_count += 1;
        self.add(stream)
    }

    /// Finish generating the minidump and return the contents.
    pub fn finish(self) -> Option<Vec<u8>> {
        let SynthMinidump {
            section,
            flags,
            stream_count,
            stream_count_label,
            stream_directory_rva,
            stream_directory,
            ..
        } = self;
        if flags.value().is_none() {
            flags.set_const(0);
        }
        // Create the stream directory.
        stream_count_label.set_const(stream_count as u64);
        section
            .mark(&stream_directory_rva)
            .append_section(stream_directory)
            .get_contents()
    }
}

impl DumpSection for Section {
    fn file_offset(&self) -> Label {
        self.start()
    }

    fn file_size(&self) -> Label {
        self.final_size()
    }
}

macro_rules! impl_dumpsection {
    ( $x:ty ) => {
        impl DumpSection for $x {
            fn file_offset(&self) -> Label {
                self.section.file_offset()
            }
            fn file_size(&self) -> Label {
                self.section.file_size()
            }
        }
    };
}

/// A stream of arbitrary data.
pub struct SimpleStream {
    /// The stream type.
    pub stream_type: u32,
    /// The stream's contents.
    pub section: Section,
}

impl Into<Section> for SimpleStream {
    fn into(self) -> Section {
        self.section
    }
}

impl_dumpsection!(SimpleStream);

impl Stream for SimpleStream {
    fn stream_type(&self) -> u32 {
        self.stream_type
    }
}

/// A stream containing a list of dump entries.
pub struct List<T: DumpSection> {
    /// The stream type.
    stream_type: u32,
    /// The stream's contents.
    section: Section,
    /// The number of entries.
    count: u32,
    /// The number of entries, as a `Label`.
    count_label: Label,
    _type: PhantomData<T>,
}

impl<T: DumpSection> List<T> {
    pub fn new(stream_type: u32, endian: Endian) -> List<T> {
        let count_label = Label::new();
        List {
            stream_type: stream_type,
            section: Section::with_endian(endian).D32(&count_label),
            count_label: count_label,
            count: 0,
            _type: PhantomData,
        }
    }

    pub fn add(mut self, entry: T) -> List<T> {
        self.count += 1;
        self.section = self.section
            .mark(&entry.file_offset())
            .append_section(entry);
        self
    }
}

impl<T: DumpSection> Into<Section> for List<T> {
    fn into(self) -> Section {
        // Finalize the entry count.
        self.count_label.set_const(self.count as u64);
        self.section
    }
}

impl<T: DumpSection> DumpSection for List<T> {
    fn file_offset(&self) -> Label {
        self.section.file_offset()
    }
    fn file_size(&self) -> Label {
        self.section.file_size()
    }
}

impl<T: DumpSection> Stream for List<T> {
    fn stream_type(&self) -> u32 {
        self.stream_type
    }
}

/// An `MDString`, a UTF-16 string preceded by a 4-byte length.
pub struct DumpString {
    section: Section,
}

impl DumpString {
    /// Create a new `DumpString` with `s` as its contents, using `endian` endianness.
    fn new(s: &str, endian: Endian) -> DumpString {
        let u16_s = UTF_16LE.encode(s, EncoderTrap::Strict).unwrap();
        let section = Section::with_endian(endian)
            .D32(u16_s.len() as u32)
            .append_bytes(&u16_s);
        DumpString {
            section: section,
        }
    }
}

impl Into<Section> for DumpString {
    fn into(self) -> Section {
        self.section
    }
}

impl_dumpsection!(DumpString);

/// MDRawMiscInfo stream.
pub struct MiscStream {
    /// The stream's contents.
    section: Section,
    pub process_id: Option<u32>,
    pub process_create_time: Option<u32>,
    pub pad_to_size: Option<usize>,
}

impl MiscStream {
    pub fn new(endian: Endian) -> MiscStream {
        let section = Section::with_endian(endian);
        let size = section.final_size();
        MiscStream {
            section: section.D32(size),
            process_id: None,
            process_create_time: None,
            pad_to_size: None,
        }
    }
}

impl Into<Section> for MiscStream {
    fn into(self) -> Section {
        let MiscStream { section, process_id, process_create_time, pad_to_size } = self;
        let flags_label = Label::new();
        let section = section.D32(&flags_label);
        let mut flags = 0;
        let section = section.D32(if let Some(pid) = process_id {
            flags = flags | md::MD_MISCINFO_FLAGS1_PROCESS_ID;
            pid
        } else { 0 });
        let section = if let Some(time) = process_create_time {
            flags = flags | md::MD_MISCINFO_FLAGS1_PROCESS_TIMES;
            section.D32(time)
                // user_time
                .D32(0)
                // kernel_time
                .D32(0)
        } else {
            section.D32(0)
                .D32(0)
                .D32(0)
        };
        flags_label.set_const(flags as u64);
        // Pad to final size, if necessary.
        if let Some(size) = pad_to_size {
            let size = (size as u64 - section.size()) as usize;
            section.append_repeated(0, size)
        } else {
            section
        }
    }
}

impl_dumpsection!(MiscStream);

impl Stream for MiscStream {
    fn stream_type(&self) -> u32 {
        md::MD_MISC_INFO_STREAM
    }
}

#[test]
fn test_dump_header() {
    let dump =
        SynthMinidump::with_endian(Endian::Little)
        .flags(0x9f738b33685cc84c);
    assert_eq!(dump.finish().unwrap(),
               vec![0x4d, 0x44, 0x4d, 0x50, // signature
                    0x93, 0xa7, 0x00, 0x00, // version
                    0, 0, 0, 0,             // stream count
                    0x20, 0, 0, 0,          // directory RVA
                    0, 0, 0, 0,             // checksum
                    0x3d, 0xe1, 0x44, 0x4b, // time_date_stamp
                    0x4c, 0xc8, 0x5c, 0x68, // flags
                    0x33, 0x8b, 0x73, 0x9f,
                    ]);
}

#[test]
fn test_dump_header_bigendian() {
    let dump =
        SynthMinidump::with_endian(Endian::Big)
        .flags(0x9f738b33685cc84c);
    assert_eq!(dump.finish().unwrap(),
               vec![0x50, 0x4d, 0x44, 0x4d, // signature
                    0x00, 0x00, 0xa7, 0x93, // version
                    0, 0, 0, 0,             // stream count
                    0, 0, 0, 0x20,          // directory RVA
                    0, 0, 0, 0,             // checksum
                    0x4b, 0x44, 0xe1, 0x3d, // time_date_stamp
                    0x9f, 0x73, 0x8b, 0x33, // flags
                    0x68, 0x5c, 0xc8, 0x4c,
                    ]);
}

#[test]
fn test_section_cite() {
    let s1 = Section::with_endian(Endian::Little).append_repeated(0, 0x0a);
    s1.start().set_const(0xff00ee11);
    let s2 = Section::with_endian(Endian::Little);
    let s2 = s1.cite_location_in(s2);
    s1.get_contents().unwrap();
    assert_eq!(s2.get_contents().unwrap(),
               vec![0x0a, 0,    0,    0,
                    0x11, 0xee, 0x00, 0xff]);
}

#[test]
fn test_dump_string() {
    let dump =
        SynthMinidump::with_endian(Endian::Little);
    let s = DumpString::new("hello", Endian::Little);
    let contents = dump.add(s).finish().unwrap();
    // Skip over the header
    assert_eq!(&contents[mem::size_of::<md::MDRawHeader>()..],
               &[0xa, 0x0, 0x0, 0x0, // length
                 b'h', 0x0, b'e', 0x0, b'l', 0x0, b'l', 0x0, b'o', 0x0]);
}

#[test]
fn test_list() {
    // Empty list
    let list = List::<DumpString>::new(0x11223344, Endian::Little);
    assert_eq!(Into::<Section>::into(list).get_contents().unwrap(),
               vec![0, 0, 0, 0]);
    let list = List::new(0x11223344, Endian::Little)
        .add(DumpString::new("a", Endian::Little))
        .add(DumpString::new("b", Endian::Little));
    assert_eq!(Into::<Section>::into(list).get_contents().unwrap(),
               vec![2, 0, 0, 0, // entry count
                    // first entry
                    0x2, 0x0, 0x0, 0x0, // length
                    b'a', 0x0,
                    // second entry
                    0x2, 0x0, 0x0, 0x0, // length
                    b'b', 0x0]);
}

#[test]
fn test_simple_stream() {
    let section = Section::with_endian(Endian::Little).D32(0x55667788);
    let stream_rva = mem::size_of::<md::MDRawHeader>() as u8;
    let directory_rva = stream_rva + section.size() as u8;
    let dump =
        SynthMinidump::with_endian(Endian::Little)
        .flags(0x9f738b33685cc84c)
        .add_stream(SimpleStream {
            stream_type: 0x11223344,
            section: section,
        });
    assert_eq!(dump.finish().unwrap(),
               vec![0x4d, 0x44, 0x4d, 0x50, // signature
                    0x93, 0xa7, 0x00, 0x00, // version
                    1, 0, 0, 0,             // stream count
                    directory_rva, 0, 0, 0, // directory RVA
                    0, 0, 0, 0,             // checksum
                    0x3d, 0xe1, 0x44, 0x4b, // time_date_stamp
                    0x4c, 0xc8, 0x5c, 0x68, // flags
                    0x33, 0x8b, 0x73, 0x9f,
                    // Stream contents
                    0x88, 0x77, 0x66, 0x55,
                    // Stream directory
                    0x44, 0x33, 0x22, 0x11, // stream type
                    4,    0,    0,    0,    // size
                    stream_rva, 0,    0,    0, // rva
                    ]);
}

#[test]
fn test_simple_stream_bigendian() {
    let section = Section::with_endian(Endian::Big).D32(0x55667788);
    let stream_rva = mem::size_of::<md::MDRawHeader>() as u8;
    let directory_rva = stream_rva + section.size() as u8;
    let dump =
        SynthMinidump::with_endian(Endian::Big)
        .flags(0x9f738b33685cc84c)
        .add_stream(SimpleStream {
            stream_type: 0x11223344,
            section: section,
        });
    assert_eq!(dump.finish().unwrap(),
               vec![0x50, 0x4d, 0x44, 0x4d, // signature
                    0x00, 0x00, 0xa7, 0x93, // version
                    0, 0, 0, 1,             // stream count
                    0, 0, 0, directory_rva, // directory RVA
                    0, 0, 0, 0,             // checksum
                    0x4b, 0x44, 0xe1, 0x3d, // time_date_stamp
                    0x9f, 0x73, 0x8b, 0x33, // flags
                    0x68, 0x5c, 0xc8, 0x4c,
                    // Stream contents
                    0x55, 0x66, 0x77, 0x88,
                    // Stream directory
                    0x11, 0x22, 0x33, 0x44, // stream type
                    0,    0,    0,    4,    // size
                    0,    0,    0, stream_rva,// rva
                    ]);
}
