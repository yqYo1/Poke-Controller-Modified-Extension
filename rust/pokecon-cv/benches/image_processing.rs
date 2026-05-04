use criterion::{Criterion, black_box, criterion_group, criterion_main};
use pokecon_cv::camera::{Frame, PixelFormat};
use pokecon_cv::image_processing::{ImageProcessor, Region};
use rand::SeedableRng;

/// Create a synthetic grayscale frame of given dimensions filled with random data.
fn create_test_frame(width: u32, height: u32, seed: u64) -> Frame {
    use rand::Rng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let data: Vec<u8> = (0..(width * height)).map(|_| rng.r#gen()).collect();
    Frame {
        width,
        height,
        data,
        format: PixelFormat::Gray,
    }
}

/// Create a synthetic RGB frame.
fn create_test_frame_rgb(width: u32, height: u32, seed: u64) -> Frame {
    use rand::Rng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let data: Vec<u8> = (0..(width * height * 3)).map(|_| rng.r#gen()).collect();
    Frame {
        width,
        height,
        data,
        format: PixelFormat::Rgb,
    }
}

// ── Crop benchmarks ─────────────────────────────────────────────────────────

fn bench_crop_small(c: &mut Criterion) {
    let frame = create_test_frame_rgb(640, 480, 42);
    let region = Region {
        x: 100,
        y: 100,
        width: 200,
        height: 200,
    };

    c.bench_function("crop_640x480_rgb_200x200", |b| {
        b.iter(|| ImageProcessor::crop(black_box(&frame), black_box(&region)))
    });
}

fn bench_crop_large(c: &mut Criterion) {
    let frame = create_test_frame_rgb(1920, 1080, 42);
    let region = Region {
        x: 500,
        y: 300,
        width: 800,
        height: 600,
    };

    c.bench_function("crop_1920x1080_rgb_800x600", |b| {
        b.iter(|| ImageProcessor::crop(black_box(&frame), black_box(&region)))
    });
}

// ── Grayscale benchmarks ────────────────────────────────────────────────────

fn bench_grayscale_small(c: &mut Criterion) {
    let frame = create_test_frame_rgb(640, 480, 42);

    c.bench_function("grayscale_640x480_rgb", |b| {
        b.iter(|| ImageProcessor::grayscale(black_box(&frame)))
    });
}

fn bench_grayscale_large(c: &mut Criterion) {
    let frame = create_test_frame_rgb(1920, 1080, 42);

    c.bench_function("grayscale_1920x1080_rgb", |b| {
        b.iter(|| ImageProcessor::grayscale(black_box(&frame)))
    });
}

// ── Template matching benchmarks ────────────────────────────────────────────

fn bench_template_match_small(c: &mut Criterion) {
    let frame = create_test_frame(320, 240, 42);
    let template = create_test_frame(32, 32, 43);

    c.bench_function("template_match_320x240_t32x32", |b| {
        b.iter(|| {
            ImageProcessor::template_match(black_box(&frame), black_box(&template), black_box(0.8))
        })
    });
}

fn bench_template_match_medium(c: &mut Criterion) {
    let frame = create_test_frame(640, 480, 42);
    let template = create_test_frame(48, 48, 43);

    c.bench_function("template_match_640x480_t48x48", |b| {
        b.iter(|| {
            ImageProcessor::template_match(black_box(&frame), black_box(&template), black_box(0.8))
        })
    });
}

// ── Threshold benchmarks ────────────────────────────────────────────────────

fn bench_threshold(c: &mut Criterion) {
    let frame = create_test_frame(640, 480, 42);

    c.bench_function("threshold_640x480", |b| {
        b.iter(|| ImageProcessor::threshold(black_box(&frame), black_box(128)))
    });
}

fn bench_in_range(c: &mut Criterion) {
    let frame = create_test_frame_rgb(640, 480, 42);

    c.bench_function("in_range_640x480_rgb", |b| {
        b.iter(|| {
            ImageProcessor::in_range(
                black_box(&frame),
                black_box([30, 30, 30]),
                black_box([220, 220, 220]),
            )
        })
    });
}

fn bench_preprocess(c: &mut Criterion) {
    use pokecon_cv::image_processing::PreprocessConfig;
    let frame = create_test_frame_rgb(640, 480, 42);
    let config = PreprocessConfig {
        crop: Some(Region {
            x: 50,
            y: 50,
            width: 400,
            height: 300,
        }),
        grayscale: true,
        binarize: None,
        threshold_binary: Some(128),
    };

    c.bench_function("preprocess_crop+gray+threshold_640x480", |b| {
        b.iter(|| ImageProcessor::preprocess(black_box(&frame), black_box(&config)))
    });
}

criterion_group!(
    benches,
    bench_crop_small,
    bench_crop_large,
    bench_grayscale_small,
    bench_grayscale_large,
    bench_template_match_small,
    bench_template_match_medium,
    bench_threshold,
    bench_in_range,
    bench_preprocess,
);
criterion_main!(benches);
