use std::io::Write;

use std::marker::PhantomData;

use arrow2::{
    datatypes::Schema,
    error::Result,
    io::parquet::write::{
        CompressionOptions, Encoding, FileWriter, RowGroupIterator, Version, WriteOptions,
    },
};

use super::EncodeToChunk;

pub struct ParquetWriter<A, W>
where
    W: Write,
{
    pub writer: FileWriter<W>,
    pub encodings: Vec<Vec<Encoding>>,
    pub options: WriteOptions,
    pub parquet_type: PhantomData<*const A>,
}

impl<'a, A, W> ParquetWriter<A, W>
where
    A: EncodeToChunk + 'a + Sized,
    W: Write,
{
    pub fn new(w: W, schema: Schema) -> ParquetWriter<A, W> {
        let options = WriteOptions {
            write_statistics: true,
            compression: CompressionOptions::Uncompressed,
            version: Version::V2,
        };
        let encodings = A::encodings(schema.metadata.clone());
        let writer = FileWriter::try_new(w, schema, options).unwrap();

        ParquetWriter {
            writer,
            encodings,
            options,
            parquet_type: PhantomData,
        }
    }

    pub fn new_buffer_writer(schema: Schema) -> ParquetWriter<A, Vec<u8>> {
        ParquetWriter::new(Vec::new(), schema)
    }

    pub fn write(&mut self, data: &[A]) -> Result<()> {
        let cols = A::encode(data.iter());
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

    pub fn write_streaming<I>(&mut self, data_stream: I) -> Result<()>
    where
        I: Iterator<Item = Vec<A>>,
    {
        let chunk_stream = data_stream.map(|chunk| Ok(A::encode(chunk.iter())));
        let row_groups = RowGroupIterator::try_new(
            chunk_stream,
            self.writer.schema(),
            self.options,
            self.encodings.clone(),
        )?;

        for group in row_groups {
            self.writer.write(group?)?;
        }
        Ok(())
    }

    pub fn flush(mut self) -> W {
        let _size = self.writer.end(None);
        self.writer.into_inner()
    }
}
