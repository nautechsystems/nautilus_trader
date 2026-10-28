// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Typed external ingress through the Python mapping and callback boundary.

use std::{fs, hint::black_box, time::Instant};

use nautilus_common::msgbus::{BusMessage, process_external_typed_message};
use nautilus_live::python::config::json_value_to_py;
use nautilus_model::{
    data::stubs::ensure_stub_custom_data_registered, instruments::stubs::tokenized_asset_aaplx,
};
use pyo3::prelude::*;

fn main() {
    let directory = std::env::var("NAUTILUS_BENCH_REPLAY_DIR").unwrap();
    let selected = std::env::var("NAUTILUS_BENCH_CASE").ok();
    let format = std::env::var("NAUTILUS_BENCH_FORMAT").ok();
    let samples = std::env::var("NAUTILUS_BENCH_SAMPLES")
        .map_or(10_000, |value| value.parse::<usize>().unwrap());
    let trials =
        std::env::var("NAUTILUS_BENCH_TRIALS").map_or(3, |value| value.parse::<usize>().unwrap());
    let warmup = std::env::var("NAUTILUS_BENCH_WARMUP")
        .map_or(1_000, |value| value.parse::<usize>().unwrap());
    assert!(samples > 0);
    let mut paths = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    paths.sort();
    // Register the currency used by the captured tokenized-asset fixture
    tokenized_asset_aaplx();
    ensure_stub_custom_data_registered();
    Python::initialize();

    let callback = Python::attach(|py| {
        py.eval(c"lambda payload: None", None, None)
            .unwrap()
            .unbind()
    });

    for trial in 0..trials {
        for index in 0..paths.len() {
            let path = &paths[(index + trial) % paths.len()];
            let fixture: serde_json::Value =
                serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
            let name = fixture["case"].as_str().unwrap();

            if selected.as_ref().is_some_and(|selected| selected != name)
                || fixture["diagnostic"].as_bool().unwrap()
            {
                continue;
            }

            let message: BusMessage = serde_json::from_value(fixture["message"].clone()).unwrap();

            if !message.payload_type.is_typed_message()
                || format.as_ref().is_some_and(|format| {
                    serde_json::to_value(message.encoding).unwrap().as_str()
                        != Some(format.as_str())
                })
            {
                continue;
            }

            measure_processor(
                name,
                &message,
                &fixture["expected"],
                &callback,
                (samples, warmup, trial),
            );
        }
    }
}

fn measure_processor(
    name: &str,
    message: &BusMessage,
    expected: &serde_json::Value,
    callback: &Py<PyAny>,
    (samples, warmup, trial): (usize, usize, usize),
) {
    let mut expected = expected.clone();
    expected.as_object_mut().unwrap().insert(
        "payload_type".into(),
        serde_json::Value::from(message.payload_type.as_str()),
    );
    process_external_typed_message(message, &mut |_, mapping| {
        Python::attach(|py| {
            let payload = json_value_to_py(py, mapping).unwrap();
            let encoded = py
                .import("json")
                .unwrap()
                .call_method1("dumps", (payload,))
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&encoded).unwrap(),
                expected
            );
        });

        Ok(())
    })
    .unwrap();

    for _ in 0..warmup {
        process_external_typed_message(message, &mut |_, mapping| {
            Python::attach(|py| {
                let payload = json_value_to_py(py, mapping).unwrap();
                callback.call1(py, (payload,)).unwrap();
            });

            Ok(())
        })
        .unwrap();
    }

    if std::env::var_os("NAUTILUS_BENCH_PROFILE").is_some() {
        for _ in 0..samples {
            call_processor(black_box(message), callback);
        }

        println!(
            "{}",
            serde_json::json!({"case": name, "format": message.encoding,
            "operation": "python_processor", "profile_calls": samples, "bytes": message.payload.len()})
        );
        return;
    }

    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        call_processor(black_box(message), callback);
        times.push(started.elapsed().as_nanos() as u64);
    }

    let mean = times.iter().map(|value| *value as f64).sum::<f64>() / samples as f64;
    times.sort_unstable();
    let percentile = |n| times[(samples - 1) * n / 1_000];
    println!(
        "{}",
        serde_json::json!({"case": name, "operation": "python_processor",
        "format": message.encoding, "trial": trial, "samples": samples,
        "bytes": message.payload.len(), "mean_ns": mean, "p50_ns": percentile(500),
        "p95_ns": percentile(950), "p99_ns": percentile(990), "p999_ns": percentile(999),
        "max_ns": times.last().unwrap()})
    );
}

#[inline(never)]
fn call_processor(message: &BusMessage, callback: &Py<PyAny>) {
    process_external_typed_message(message, &mut |_, mapping| {
        Python::attach(|py| {
            let payload = json_value_to_py(py, mapping).unwrap();
            callback.call1(py, (payload,)).unwrap();
        });

        Ok(())
    })
    .unwrap();
}
