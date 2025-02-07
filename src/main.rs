use anyhow::Result;
use clap::Parser;
use image::{ImageBuffer, Rgb};
use resvg::{self, usvg};
use std::path::PathBuf;
use tiny_skia::{Pixmap, PremultipliedColorU8};
use usvg::Transform;

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    /// Path to the input SVG file
    input: PathBuf,
    /// Path to the output PNG file
    #[arg(short, long, default_value = "output.png")]
    output: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Loading and parsing the SVG file
    let svg_data = std::fs::read(&cli.input)?;
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(&svg_data, &opt)?;

    // Define target size
    // TODO: (ROB) make this a CLI argument
    let target_width = 400;
    let target_height = 400;

    // Calculate scaling factors
    let scale_x = target_width as f32 / tree.size().width();
    let scale_y = target_height as f32 / tree.size().height();
    // Use the smaller scale to fit the entire SVG
    let scale = scale_x.min(scale_y);

    // Create Pixmap with target size
    let mut pixmap = Pixmap::new(target_width, target_height)
        .ok_or_else(|| anyhow::anyhow!("Couldn't create the Pixmap"))?;

    // Create a transform that scales and centers the SVG
    // let transform = Transform::from_scale(scale, scale)
    //     .post_scale(
    //         (target_width as f32 - tree.size().width() * scale) / 2.0,
    //         (target_height as f32 - tree.size().height() * scale) / 2.0,
    //     );

    let transform = Transform::from_scale(scale, scale);

    // Render SVG with scaling
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // Create image (dark + light)
    let mut combined = ImageBuffer::<Rgb<u8>, _>::new(target_width * 2, target_height);

    let white_background = PremultipliedColorU8::from_rgba(255, 255, 255, 255);

    for y in 0..target_height {
        for x in 0..target_width {
            if let Some(pixel) = pixmap.pixel(x, y) {
                // dark background (original)
                combined.put_pixel(x, y, blend(pixel, None));

                // white background
                combined.put_pixel(x + target_width, y, blend(pixel, white_background));
            }
        }
    }

    // saving the PNG
    combined.save(cli.output)?;

    Ok(())
}

fn blend(pixel: PremultipliedColorU8, background: Option<PremultipliedColorU8>) -> Rgb<u8> {
    if let Some(background) = background {
        let a = pixel.alpha() as f32 / 255.0;
        let blend_color = |c: u8, bg: u8| ((c as f32 * a) + (bg as f32 * (1.0 - a))) as u8;
        Rgb([
            blend_color(pixel.red(), background.red()),
            blend_color(pixel.green(), background.green()),
            blend_color(pixel.blue(), background.blue()),
        ])
    } else {
        Rgb([pixel.red(), pixel.green(), pixel.blue()])
    }
}
