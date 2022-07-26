pub mod parquet_quote_tick;

use std::{fs::File, marker::PhantomData, sync::Arc};

use arrow2::{
    array::Array,
    chunk::Chunk,
    datatypes::Schema,
    error::Result,
    io::parquet::{
        read::FileReader,
        write::{
            CompressionOptions, Encoding, FileWriter, RowGroupIterator, Version, WriteOptions,
        },
    },
};

pub struct ParquetReader<A> {
    file_reader: FileReader<File>,
    reader_type: PhantomData<*const A>,
}

impl<A> ParquetReader<A> {
    pub fn new(file_path: &str, chunk_size: usize) -> Self {
        let file = File::open(file_path)
            .expect(format!("Unable to open parquet file {file_path}").as_str());
        let fr = FileReader::try_new(file, None, Some(chunk_size), None, None)
            .expect("Unable to create reader from file")
            .into_iter();
        ParquetReader {
            file_reader: fr,
            reader_type: PhantomData,
        }
    }
}

impl<A> Iterator for ParquetReader<A>
where
    A: DecodeFromChunk,
{
    type Item = Vec<A>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(Ok(chunk)) = self.file_reader.next() {
            Some(A::decode(self.file_reader.schema(), chunk))
        } else {
            None
        }
    }
}

pub struct ParquetWriter<A> {
    pub writer: FileWriter<File>,
    pub encodings: Vec<Vec<Encoding>>,
    pub options: WriteOptions,
    pub writer_type: PhantomData<*const A>,
}

impl<A> ParquetWriter<A>
where
    A: EncodeToChunk,
{
    pub fn new(path: &str, schema: Schema) -> Self {
        let options = WriteOptions {
            write_statistics: true,
            compression: CompressionOptions::Uncompressed,
            version: Version::V2,
        };

        let encodings = A::encodings();

        // Create a new empty file
        let file = File::create(path).unwrap();

        let writer = FileWriter::try_new(file, schema, options).unwrap();

        ParquetWriter {
            writer,
            encodings,
            options,
            writer_type: PhantomData,
        }
    }

    pub fn write_bulk<I>(self: &mut Self, data_stream: I) -> Result<()>
    where
        I: Iterator<Item = Vec<A>>,
    {
        let chunk_stream = data_stream.map(|chunk| Ok(A::encode(chunk)));
        let row_groups = RowGroupIterator::try_new(
            chunk_stream,
            self.writer.schema(),
            self.options,
            self.encodings.clone(),
        )?;

        for group in row_groups {
            self.writer.write(group?)?;
        }
        let _size = self.writer.end(None);
        Ok(())
    }

    pub fn write(self: &mut Self, data: Vec<A>) -> Result<()> {
        let cols = A::encode(data);
        let iter = vec![Ok(cols)];
        let row_groups = RowGroupIterator::try_new(
            iter.into_iter(),
            self.writer.schema(),
            self.options,
            self.encodings.clone(),
        )?;

        for group in row_groups {
            self.writer.write(group?)?;
        }
        Ok(())
    }

    pub fn end_writer(self: &mut Self) {
        let _size = self.writer.end(None);
    }
}

pub trait DecodeFromChunk
where
    Self: Sized,
{
    fn decode(schema: &Schema, cols: Chunk<Arc<dyn Array>>) -> Vec<Self>;
}

pub trait EncodeToChunk
where
    Self: Sized,
{
    fn encodings() -> Vec<Vec<Encoding>>;
    fn encode_schema() -> Schema;
    fn encode(data: Vec<Self>) -> Chunk<Box<dyn Array>>;
}
