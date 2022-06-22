use std::fs::File;

use arrow2::{
    array::{Array, BooleanArray, UInt64Array},
    chunk::Chunk,
    datatypes::{DataType, Field, Schema},
    error::Result,
    io::parquet::{
        read::FileReader,
        write::{
            transverse, CompressionOptions, Encoding, FileWriter, RowGroupIterator, Version,
            WriteOptions,
        },
    },
};

fn write_batch(path: &str, schema: Schema, columns: Chunk<Box<dyn Array>>) -> Result<()> {
    let options = WriteOptions {
        write_statistics: true,
        compression: CompressionOptions::Uncompressed,
        version: Version::V2,
    };

    let iter = vec![Ok(columns)];

    let encodings = schema
        .fields
        .iter()
        .map(|f| transverse(&f.data_type, |_| Encoding::Plain))
        .collect();

    let row_groups = RowGroupIterator::try_new(iter.into_iter(), &schema, options, encodings)?;

    // Create a new empty file
    let file = File::create(path)?;

    let mut writer = FileWriter::try_new(file, schema, options)?;

    for group in row_groups {
        writer.write(group?)?;
    }
    let _size = writer.end(None)?;
    Ok(())
}

struct Value {
    a: u64,
    b: bool,
}

fn write() {
    let values = vec![
        Value { a: 1, b: true },
        Value { a: 2, b: false },
        Value { a: 3, b: false },
        Value { a: 4, b: true },
    ];

    let a_array = UInt64Array::from_slice(values.iter().map(|v| v.a).collect::<Vec<u64>>()).arced();
    let b_array =
        BooleanArray::from_slice(values.iter().map(|v| v.b).collect::<Vec<bool>>()).arced();

    let fields = vec![
        Field::new("a", DataType::UInt64, false),
        Field::new("b", DataType::Boolean, false),
    ];

    // let array = StructArray::new(
    //     DataType::Struct(fields.clone()),
    //     vec![a_array, b_array],
    //     None,
    // );
    // let schema = Schema::from(vec![Field::new("bid", DataType::Struct(fields), false)]);
    let schema = Schema::from(fields);
    let columns = Chunk::new(vec![a_array.to_boxed(), b_array.to_boxed()]);
    write_batch("test.parquet", schema, columns).unwrap();
}

fn read() {
    let fields = vec![
        Field::new("a", DataType::UInt64, false),
        Field::new("b", DataType::Boolean, false),
    ];

    let f = File::open("test.parquet").unwrap();
    let fr = FileReader::try_new(&f, None, None, None, None).unwrap();
    // let schema = Schema::from(vec![Field::new("bid", DataType::Struct(fields), false)]);

    for chunk in fr.into_iter() {
        if let Ok(cols) = chunk {
            for array in cols.arrays().iter() {
                dbg!(array);
            }
        }
    }
}

fn main() {
    read();
}
