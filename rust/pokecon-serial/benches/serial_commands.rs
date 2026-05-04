use criterion::{Criterion, black_box, criterion_group, criterion_main};
use pokecon_serial::format::SendFormat;
use pokecon_serial::keypress::KeyPress;
use pokecon_serial::keys::{Button, Direction, Hat, Stick};
use pokecon_serial::sender::Sender;

// ── SendFormat benchmarks (no serial I/O) ───────────────────────────────────

fn bench_send_format_new(c: &mut Criterion) {
    c.bench_function("send_format_new", |b| b.iter(SendFormat::new));
}

fn bench_send_format_set_button(c: &mut Criterion) {
    let mut fmt = SendFormat::new();

    c.bench_function("send_format_set_button_A", |b| {
        b.iter(|| {
            fmt.reset();
            fmt.set_button(black_box(&[Button::A]));
        })
    });
}

fn bench_send_format_set_button_combo(c: &mut Criterion) {
    let mut fmt = SendFormat::new();

    c.bench_function("send_format_set_button_A_B_X", |b| {
        b.iter(|| {
            fmt.reset();
            fmt.set_button(black_box(&[Button::A, Button::B, Button::X]));
        })
    });
}

fn bench_send_format_set_hat(c: &mut Criterion) {
    let mut fmt = SendFormat::new();

    c.bench_function("send_format_set_hat_TOP", |b| {
        b.iter(|| {
            fmt.reset();
            fmt.set_hat(black_box(&[Hat::TOP]));
        })
    });
}

fn bench_send_format_set_direction(c: &mut Criterion) {
    let mut fmt = SendFormat::new();
    let dir = Direction::from_xy(Stick::Left, 128, 128);

    c.bench_function("send_format_set_direction_L128_128", |b| {
        b.iter(|| {
            fmt.reset();
            fmt.set_any_direction(black_box(std::slice::from_ref(&dir)));
        })
    });
}

fn bench_send_format_convert_default(c: &mut Criterion) {
    let mut fmt = SendFormat::new();
    fmt.set_button(&[Button::A, Button::B]);
    fmt.set_hat(&[Hat::TOP]);

    c.bench_function("send_format_convert_default_A+B+TOP", |b| {
        b.iter(|| fmt.convert_to_default(black_box(false), black_box(false)))
    });
}

fn bench_send_format_convert_default_with_stick(c: &mut Criterion) {
    let mut fmt = SendFormat::new();
    fmt.set_button(&[Button::A]);
    fmt.set_any_direction(&[Direction::from_xy(Stick::Left, 100, 150)]);

    c.bench_function("send_format_convert_default_A+L(100,150)", |b| {
        b.iter(|| fmt.convert_to_default(black_box(true), black_box(false)))
    });
}

fn bench_send_format_convert_qingpi(c: &mut Criterion) {
    let mut fmt = SendFormat::new();
    fmt.set_button(&[Button::A, Button::X]);

    c.bench_function("send_format_convert_qingpi_A+X", |b| {
        b.iter(|| fmt.convert_to_qingpi())
    });
}

fn bench_send_format_convert_3ds(c: &mut Criterion) {
    let mut fmt = SendFormat::new();
    fmt.set_button_3ds_bits(1 | 2 | 4); // A + B + X in 3DS mode

    c.bench_function("send_format_convert_3ds_A+B+X", |b| {
        b.iter(|| fmt.convert_to_3ds())
    });
}

fn bench_send_format_full_pipeline(c: &mut Criterion) {
    c.bench_function("send_format_full_pipeline", |b| {
        b.iter(|| {
            let mut fmt = SendFormat::new();
            fmt.set_button(black_box(&[Button::A, Button::B]));
            fmt.set_hat(black_box(&[Hat::TOP]));
            fmt.set_any_direction(black_box(&[Direction::from_xy(Stick::Left, 200, 50)]));
            let _row = fmt.convert_to_default(true, false);
            let _qingpi = fmt.convert_to_qingpi();
        })
    });
}

// ── KeyPress construction benchmarks (no async I/O) ─────────────────────────

fn bench_keypress_new(c: &mut Criterion) {
    c.bench_function("keypress_new", |b| {
        b.iter(|| {
            let sender = Sender::new(false);
            KeyPress::new(sender)
        })
    });
}

criterion_group!(
    benches,
    bench_send_format_new,
    bench_send_format_set_button,
    bench_send_format_set_button_combo,
    bench_send_format_set_hat,
    bench_send_format_set_direction,
    bench_send_format_convert_default,
    bench_send_format_convert_default_with_stick,
    bench_send_format_convert_qingpi,
    bench_send_format_convert_3ds,
    bench_send_format_full_pipeline,
    bench_keypress_new,
);
criterion_main!(benches);
