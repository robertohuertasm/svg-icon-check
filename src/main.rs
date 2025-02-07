use anyhow::Result;
use clap::Parser;
use image::{ImageBuffer, Rgb};
use resvg::{self, usvg};
use std::{path::PathBuf, vec};
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
    /// Width of the output icon
    #[arg(short = 'w', long, default_value = "200")]
    icon_width: u32,
    /// Additional background colors in RGB format (e.g., 255,0,0)
    #[arg(short, long, value_delimiter = ' ', num_args = 1..)]
    backgrounds: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Loading and parsing the SVG file
    let svg_data = std::fs::read(&cli.input)?;
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(&svg_data, &opt)?;

    // Calculate scaling factors
    let icon_width = cli.icon_width;
    let scale_x = icon_width as f32 / tree.size().width();
    let scale_y = icon_width as f32 / tree.size().height();
    // Use the smaller scale to fit the entire SVG
    let scale = scale_x.min(scale_y);

    // Create Pixmap with target size
    let mut pixmap = Pixmap::new(icon_width, icon_width)
        .ok_or_else(|| anyhow::anyhow!("Couldn't create the Pixmap"))?;

    let transform = Transform::from_scale(scale, scale);

    // Render SVG with scaling
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let mut bg_colors = vec![PremultipliedColorU8::from_rgba(255, 255, 255, 255)];

    for bg in cli.backgrounds {
        let rgb: Vec<u8> = bg
            .split(',')
            .map(|c| c.parse().expect("Invalid color format"))
            .collect();

        if rgb.len() != 3 {
            panic!("Invalid RGB format: {}", bg);
        }

        bg_colors.push(PremultipliedColorU8::from_rgba(rgb[0], rgb[1], rgb[2], 255));
    }

    // Create image with the width of all the backgrounds
    let mut combined =
        ImageBuffer::<Rgb<u8>, _>::new(icon_width * (bg_colors.len() as u32 + 1), icon_width);

    for y in 0..icon_width {
        for x in 0..icon_width {
            if let Some(pixel) = pixmap.pixel(x, y) {
                // dark background (original)
                combined.put_pixel(x, y, blend(&pixel, None));

                // additional backgrounds
                for (i, background) in bg_colors.iter().enumerate() {
                    combined.put_pixel(
                        x + icon_width * (i as u32 + 1),
                        y,
                        blend(&pixel, *background),
                    );
                }
            }
        }
    }

    // saving the PNG
    combined.save(cli.output)?;

    Ok(())
}

fn blend(pixel: &PremultipliedColorU8, background: Option<PremultipliedColorU8>) -> Rgb<u8> {
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
