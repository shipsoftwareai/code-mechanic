// Lines above and below the targets model the context an agent receives from a
// conventional search followed by a source-range read.
// context 01
// context 02
// context 03
// context 04
// context 05
// context 06
// context 07
// context 08
// context 09
// context 10
// context 11
// context 12
// context 13
// context 14
// context 15
// context 16
// context 17
// context 18
// context 19
// context 20
// context 21
// context 22
// context 23
// context 24

fn simple_probe(value: usize) -> usize {
    value + 1
}

fn use_simple_probe() -> usize {
    simple_probe(4)
}

fn use_simple_probe_again() -> usize {
    crate::simple_probe(8)
}

// simple_probe() stays documentary text.
const DOCUMENT: &str = "simple_probe() stays string data";

struct Worker;

impl Worker {
    #[inline]
    async fn rust_complex<'a, T>(&self, values: &'a [T], limit: usize) -> usize
    where
        T: Send + Sync,
    {
        let capped = limit.min(values.len());
        let mut accepted = 0usize;
        let mut weighted_total = 0usize;

        for (index, value) in values.iter().take(capped).enumerate() {
            let measured = consume(value).await;
            if measured == 0 {
                continue;
            }

            let weighted = measured.saturating_mul(index + 1);
            weighted_total = weighted_total.saturating_add(weighted);
            if weighted % 2 == 0 || index == 0 {
                accepted += 1;
            }
        }

        match accepted {
            0 => 0,
            count => weighted_total / count,
        }
    }
}

async fn consume<T>(_value: &T) -> usize {
    7
}

async fn use_rust_complex(worker: &Worker) -> usize {
    worker.rust_complex(&[9_u32, 11, 13, 15], 3).await
}

// trailing context 01
// trailing context 02
// trailing context 03
// trailing context 04
// trailing context 05
// trailing context 06
// trailing context 07
// trailing context 08
// trailing context 09
// trailing context 10
// trailing context 11
// trailing context 12
// trailing context 13
// trailing context 14
// trailing context 15
// trailing context 16
// trailing context 17
// trailing context 18
// trailing context 19
// trailing context 20
// trailing context 21
// trailing context 22
// trailing context 23
// trailing context 24
