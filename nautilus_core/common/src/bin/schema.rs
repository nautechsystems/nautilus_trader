use std::fs::File;

use arrow2::{
    array::{Array, BooleanArray, StructArray, UInt64Array, Utf8Array},
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

#[derive(Debug)]
struct InnerValue {
    c: String,
}

#[derive(Debug)]
struct Value {
    a: u64,
    b: bool,
    d: InnerValue,
}

fn write_struct_array() {
    //////////////////////////////////////
    // Write parquet
    //////////////////////////////////////

    let values = vec![
        Value {
            a: 1,
            b: true,
            d: InnerValue {
                c: "hi".to_string(),
            },
        },
        Value {
            a: 2,
            b: false,
            d: InnerValue {
                c: "hola".to_string(),
            },
        },
        Value {
            a: 3,
            b: false,
            d: InnerValue {
                c: "bola".to_string(),
            },
        },
        Value {
            a: 4,
            b: true,
            d: InnerValue {
                c: "chola".to_string(),
            },
        },
    ];

    let a_array = UInt64Array::from_slice(values.iter().map(|v| v.a).collect::<Vec<u64>>()).arced();
    let b_array =
        BooleanArray::from_slice(values.iter().map(|v| v.b).collect::<Vec<bool>>()).arced();
    let d_array = Utf8Array::<i32>::from_slice(
        values
            .iter()
            .map(|v| v.d.c.clone())
            .collect::<Vec<String>>(),
    )
    .arced();

    let fields = vec![
        Field::new("a", DataType::UInt64, false),
        Field::new("b", DataType::Boolean, false),
        Field::new("d", DataType::Utf8, false),
    ];

    let array = StructArray::new(
        DataType::Struct(fields.clone()),
        vec![a_array, b_array, d_array],
        None,
    );
    let schema = Schema::from(vec![Field::new("bid", DataType::Struct(fields), false)]);
    let columns = Chunk::new(vec![array.boxed()]);
    write_batch("struct.parquet", schema, columns).unwrap();

    //////////////////////////////////////
    // Read parquet
    //////////////////////////////////////

    let f = File::open("struct.parquet").unwrap();
    let fr = FileReader::try_new(&f, None, None, None, None).unwrap();

    for chunk in fr.into_iter() {
        if let Ok(cols) = chunk {
            for array in cols.arrays().iter() {
                match array.data_type().to_physical_type() {
                    // convert array to struct array
                    arrow2::datatypes::PhysicalType::Struct => {
                        let struct_array = array.as_any().downcast_ref::<StructArray>().unwrap();
                        dbg!(struct_array);

                        // deconstruct individual field arrays from struct array
                        let values = struct_array.values();
                        let a_values = values[0].as_any().downcast_ref::<UInt64Array>().unwrap();
                        let b_values = values[1].as_any().downcast_ref::<BooleanArray>().unwrap();
                        let d_values = values[2].as_any().downcast_ref::<Utf8Array<i32>>().unwrap();

                        // construct iterator of values from field value arrays
                        let values = a_values
                            .into_iter()
                            .zip(b_values.into_iter())
                            .zip(d_values.into_iter())
                            .map(|((a, b), d)| Value {
                                a: *a.unwrap(),
                                b: b.unwrap(),
                                d: InnerValue {
                                    c: d.unwrap().to_string(),
                                },
                            });

                        // collect vector of values if needed
                        let vec_values: Vec<Value> = values.collect();
                        dbg!(vec_values);
                    }
                    _ => todo!(),
                }
            }
        }
    }
}

fn write_array_of_arrays() {
    let values = vec![
        Value {
            a: 1,
            b: true,
            d: InnerValue {
                c: "hi".to_string(),
            },
        },
        Value {
            a: 2,
            b: false,
            d: InnerValue {
                c: "hola".to_string(),
            },
        },
        Value {
            a: 3,
            b: false,
            d: InnerValue {
                c: "bola".to_string(),
            },
        },
        Value {
            a: 4,
            b: true,
            d: InnerValue {
                c: "chola".to_string(),
            },
        },
    ];

    let a_array = UInt64Array::from_slice(values.iter().map(|v| v.a).collect::<Vec<u64>>()).arced();
    let b_array =
        BooleanArray::from_slice(values.iter().map(|v| v.b).collect::<Vec<bool>>()).arced();
    let d_array = Utf8Array::<i32>::from_slice(
        values
            .iter()
            .map(|v| v.d.c.clone())
            .collect::<Vec<String>>(),
    )
    .arced();

    let fields = vec![
        Field::new("a", DataType::UInt64, false),
        Field::new("b", DataType::Boolean, false),
        Field::new("d", DataType::Utf8, false),
    ];

    let schema = Schema::from(fields);
    let columns = Chunk::new(vec![
        a_array.to_boxed(),
        b_array.to_boxed(),
        d_array.to_boxed(),
    ]);
    write_batch("array_of_arrays.parquet", schema, columns).unwrap();

    let f = File::open("array_of_arrays.parquet").unwrap();
    let fr = FileReader::try_new(&f, None, None, None, None).unwrap();

    for chunk in fr.into_iter() {
        if let Ok(cols) = chunk {
            // extract field value arrays from chunk separately
            let a_values = cols.arrays()[0]
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap();
            let b_values = cols.arrays()[1]
                .as_any()
                .downcast_ref::<BooleanArray>()
                .unwrap();
            let d_values = cols.arrays()[2]
                .as_any()
                .downcast_ref::<Utf8Array<i32>>()
                .unwrap();

            // construct iterator of values from field value arrays
            let values = a_values
                .into_iter()
                .zip(b_values.into_iter())
                .zip(d_values.into_iter())
                .map(|((a, b), d)| Value {
                    a: *a.unwrap(),
                    b: b.unwrap(),
                    d: InnerValue {
                        c: d.unwrap().to_string(),
                    },
                });

            // collect vector of values if needed
            let vec_values: Vec<Value> = values.collect();
            dbg!(vec_values);
        }
    }
}

fn main() {
    write_struct_array();
    write_array_of_arrays();
}
