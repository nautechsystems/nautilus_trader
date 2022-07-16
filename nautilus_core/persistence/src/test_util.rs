use nautilus_model::types::fixed::f64_to_fixed_u64;
use nautilus_model::types::fixed::f64_to_fixed_i64;
use rand::Rng;

fn random_values_u64(len: u64) -> Vec<u64>{
    let mut rng = rand::thread_rng();
    let mut vec = Vec::new();
    for _ in 0..len {
        let value = f64_to_fixed_u64(rng.gen_range(1.2..1.5) as f64, 5);
        vec.push(value);
    }
    assert_eq!(vec.len() as u64, len);
    vec
}
fn random_values_i64(len: u64) -> Vec<i64>{
    let mut rng = rand::thread_rng();
    let mut vec = Vec::new();
    for _ in 0..len {
        let value = f64_to_fixed_i64(rng.gen_range(1.2..1.5)  as f64, 5);
        vec.push(value);
    }
    assert_eq!(vec.len() as u64, len);
    vec
}
fn random_values_u8(len: u64) -> Vec<u8>{
    let mut rng = rand::thread_rng();
    let mut vec = Vec::new();
    for _ in 0..len {
        let value = rng.gen_range(2..5) as u8;
        vec.push(value);
    }
    assert_eq!(vec.len() as u64, len);
    vec
}

fn date_range(len: u64) -> Vec<u64>{
    let mut vec = Vec::new();
    let mut start: u64 = 1546304400000000000;
    let end: u64 = 1577840400000000000;
    let step = (end - start) / len;
    for i in 0..len {
        start += step;
        vec.push(start);
    }
    vec
}
