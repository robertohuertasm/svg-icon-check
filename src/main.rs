use std::{
    collections::HashMap,
    env, fs, io,
    path::{absolute, Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use clap::{crate_authors, value_parser, Parser};
use icu_casemap::CaseMapper;
use icu_normalizer::DecomposingNormalizer;
use image::{ImageBuffer, Rgb};
use resvg::{self, usvg};
use tiny_skia::{ColorU8, Pixmap, PremultipliedColorU8};
use usvg::Transform;

const ALPHA_OPAQUE: u8 = 255;

#[derive(Parser)]
#[command(
    about,
    author = crate_authors!("\n  "),
    version,
    override_usage = "svg-icon-check <INPUT>... [OPTIONS]",
    help_template = concat!(
        "Description:\n {about}\n\n",
        "Authors:\n  {author}\n\n",
        "Usage:\n  {usage}\n\n",
        "Arguments:\n{positionals}\n\n",
        "{options}"
    )
)]
struct Cli {
    #[arg(
        required = true,
        help = "One or more SVG paths, or .txt files containing one SVG path per line"
    )]
    input: Vec<PathBuf>,
    #[arg(
        short = 'o',
        long = "output",
        default_value = "output",
        help = "Path to the output directory"
    )]
    output: PathBuf,
    #[arg(
        short = 'w',
        long = "icon-width",
        default_value = "200",
        value_parser = value_parser!(u32).range(1..=200),
        help = "Width of the output icon"
    )]
    icon_width: u32,
    #[arg(
        short = 'b',
        long = "backgrounds",
        value_delimiter = ' ',
        num_args = 1..,
        help = "Additional background colors in RGB format (e.g., 255,0,0)"
    )]
    backgrounds: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Performs also check for input files
    let input_files = get_input_files(&cli.input)?;

    // Perfomrns also check for background colors
    let background_colors = get_background_colors(&cli.backgrounds)?;

    // Create a temporary directory for storing preview images
    let temp_dir = tempfile::tempdir().context("Cannot create temporary directory")?;

    // Generate preview images for each input file and background color combination
    let output_dir = create_previews(
        &input_files,
        &background_colors,
        cli.icon_width,
        temp_dir.path(),
    )
    .and_then(|()| finalize_output(temp_dir.path(), cli.output))?;

    if let Err(error) = temp_dir.close() {
        eprintln!("⚠ Cannot remove temp directory: {error}");
    }

    println!(
        "✔ Generated {} preview{} in {}",
        input_files.len(),
        if input_files.len() == 1 { "" } else { "s" },
        output_dir.display()
    );

    Ok(())
}

fn get_input_files(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut filestems: HashMap<String, PathBuf> = HashMap::new();
    let case_mapper = CaseMapper::new();
    let normalizer = DecomposingNormalizer::new_nfd();
    let mut add_file = |path: &PathBuf, files: &mut Vec<PathBuf>| -> Result<()> {
        let filestem = get_filestem(path)?;
        let decomposed = normalizer.normalize(&filestem);
        let folded = case_mapper.fold_string(&decomposed);
        let key = normalizer.normalize(&folded).into_owned();
        if filestems.contains_key(&key) {
            eprintln!("⚠ Skipping duplicate filename: {}", get_filename(path)?);
            return Ok(());
        }
        let absolute_path =
            absolute(path).with_context(|| format!("Cannot resolve path: {}", path.display()))?;
        filestems.insert(key, path.clone());
        files.push(absolute_path);
        Ok(())
    };

    let unique_files = inputs.iter().try_fold(Vec::new(), |mut files, input| {
        match input.extension().and_then(|ext| ext.to_str()) {
            Some(ext) if ext.eq_ignore_ascii_case("svg") => add_file(input, &mut files)?,
            Some(ext) if ext.eq_ignore_ascii_case("txt") => {
                let base_dir = input.parent().unwrap_or_else(|| Path::new("."));
                let contents = fs::read_to_string(input).map_err(|error| {
                    let message = match error.kind() {
                        io::ErrorKind::NotFound => "TXT file does not exist",
                        io::ErrorKind::PermissionDenied => "Permission denied reading TXT file",
                        io::ErrorKind::IsADirectory => "TXT path points to a directory",
                        _ => "Cannot read TXT file",
                    };
                    anyhow::Error::new(error).context(format!("{message}: {}", input.display()))
                })?;

                contents
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .try_for_each(|line| add_file(&base_dir.join(line), &mut files))?;
            }
            _ => bail!("Unsupported file type: {}", input.display()),
        }

        Ok(files)
    })?;

    Ok(unique_files)
}

fn get_background_colors(backgrounds: &[String]) -> Result<Vec<PremultipliedColorU8>> {
    if backgrounds.len() > 16 {
        bail!("At most 16 additional background colors are allowed");
    }

    let default_colors = [
        ColorU8::from_rgba(0, 0, 0, ALPHA_OPAQUE).premultiply(),
        ColorU8::from_rgba(255, 255, 255, ALPHA_OPAQUE).premultiply(),
    ];
    let mut background_colors = default_colors.to_vec();

    for background in backgrounds {
        let rgb: [u8; 3] = background
            .split(',')
            .map(|c| c.trim().parse::<u8>())
            .collect::<Result<Vec<u8>, _>>()
            .map_err(anyhow::Error::from)
            .and_then(|rgb| {
                rgb.try_into()
                    .map_err(|_| anyhow::anyhow!("Expected exactly three RGB values"))
            })
            .context(format!("Invalid RGB format: {background}"))?;

        let color = ColorU8::from_rgba(rgb[0], rgb[1], rgb[2], ALPHA_OPAQUE).premultiply();

        if default_colors.contains(&color) {
            eprintln!("⚠ Skipping already included by default background color {background}");
            continue;
        }

        if background_colors.contains(&color) {
            eprintln!("⚠ Skipping duplicate background color: {background}");
            continue;
        }

        background_colors.push(color);
    }

    Ok(background_colors)
}

fn create_previews(
    input_files: &[PathBuf],
    background_colors: &[PremultipliedColorU8],
    icon_width: u32,
    temp_dir: &Path,
) -> Result<()> {
    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();

    for input_file in input_files {
        let filestem = get_filestem(input_file)?;

        // Loading and parsing the SVG file
        let svg_data = fs::read(input_file).map_err(|error| {
            let message = match error.kind() {
                io::ErrorKind::NotFound => "SVG file does not exist",
                io::ErrorKind::PermissionDenied => "Permission denied reading SVG file",
                io::ErrorKind::IsADirectory => "SVG path points to a directory",
                _ => "Cannot read SVG file",
            };
            anyhow::Error::new(error).context(format!("{message}: {}", input_file.display()))
        })?;

        opt.resources_dir = input_file.parent().map(Path::to_path_buf);

        let tree = usvg::Tree::from_data(&svg_data, &opt)
            .context(format!("Cannot parse SVG file: {}", input_file.display()))?;

        // Calculate scaling factors
        // Icon widths are far below 2^23, so the conversion to f32 is lossless
        #[allow(clippy::cast_precision_loss)]
        let width = icon_width as f32;

        // Create Pixmap with target size
        let mut pixmap = Pixmap::new(icon_width, icon_width).expect("Invalid icon width");

        let transform = fit_transform(tree.size().width(), tree.size().height(), width);

        // Render SVG with scaling
        resvg::render(&tree, transform, &mut pixmap.as_mut());

        let img_data = render_with_backgrounds(&pixmap, background_colors, icon_width);

        let temp_path = temp_dir.join(format!("{filestem}.png"));

        // Save the PNG image to the temp directory
        img_data.save(temp_path).context("Cannot save image")?;
    }

    Ok(())
}

fn fit_transform(svg_width: f32, svg_height: f32, canvas_width: f32) -> Transform {
    let scale = (canvas_width / svg_width).min(canvas_width / svg_height);
    let offset_x = (canvas_width - svg_width * scale) / 2.0;
    let offset_y = (canvas_width - svg_height * scale) / 2.0;

    Transform::from_scale(scale, scale).post_translate(offset_x, offset_y)
}

fn get_filestem(path: &Path) -> Result<String> {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .context(format!("Path has no filename: {}", path.display()))
}

fn get_filename(path: &Path) -> Result<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .context(format!("Path has no filename: {}", path.display()))
}

fn render_with_backgrounds(
    pixmap: &Pixmap,
    background_colors: &[PremultipliedColorU8],
    icon_width: u32,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let color_count = u32::try_from(background_colors.len()).expect("Too many background colors");
    let mut combined = ImageBuffer::<Rgb<u8>, _>::new(icon_width * color_count, icon_width);

    for (pixel_idx, pixel) in (0u32..).zip(pixmap.pixels()) {
        let x_axis = pixel_idx % icon_width;
        let y_axis = pixel_idx / icon_width;
        for (idx, background) in (0u32..).zip(background_colors) {
            combined.put_pixel(
                x_axis + icon_width * idx,
                y_axis,
                blend(*pixel, *background),
            );
        }
    }

    combined
}

fn blend(pixel: PremultipliedColorU8, background: PremultipliedColorU8) -> Rgb<u8> {
    let a = f32::from(pixel.alpha()) / f32::from(ALPHA_OPAQUE);
    // With a in 0..=1, the result is always within 0..=255, so the cast cannot
    // truncate or lose the sign
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let blend_colors = |c: u8, bg: u8| (f32::from(c) + (f32::from(bg) * (1.0 - a))) as u8;
    Rgb([
        blend_colors(pixel.red(), background.red()),
        blend_colors(pixel.green(), background.green()),
        blend_colors(pixel.blue(), background.blue()),
    ])
}

fn finalize_output(temp_dir: &Path, output: PathBuf) -> Result<PathBuf> {
    let cwd = env::current_dir()?;
    let output_abs = match fs::canonicalize(&output) {
        Ok(path) => path,
        Err(_) => cwd.join(output),
    };

    // Create the output directory
    fs::create_dir_all(&output_abs)
        .with_context(|| format!("Cannot create output directory: {}", output_abs.display()))?;

    // Copy files from temp to output
    let entries = fs::read_dir(temp_dir).context("Cannot read temp directory")?;
    for entry in entries {
        let entry = entry?;
        let target = output_abs.join(entry.file_name());

        fs::copy(entry.path(), &target).with_context(|| {
            format!("Cannot copy file: {}", entry.file_name().to_string_lossy())
        })?;
    }

    Ok(output_abs)
}

#[cfg(test)]
mod tests {
    use tiny_skia::Point;

    use super::fit_transform;

    #[test]
    fn fit_transform_centers_landscape_and_portrait_svgs() {
        let cases = [
            (
                400.0,
                200.0,
                Point::from_xy(0.0, 50.0),
                Point::from_xy(200.0, 150.0),
            ),
            (
                200.0,
                400.0,
                Point::from_xy(50.0, 0.0),
                Point::from_xy(150.0, 200.0),
            ),
        ];

        for (svg_width, svg_height, expected_top_left, expected_bottom_right) in cases {
            let transform = fit_transform(svg_width, svg_height, 200.0);
            let mut top_left = Point::from_xy(0.0, 0.0);
            let mut bottom_right = Point::from_xy(svg_width, svg_height);
            transform.map_point(&mut top_left);
            transform.map_point(&mut bottom_right);

            assert_eq!(top_left, expected_top_left);
            assert_eq!(bottom_right, expected_bottom_right);
        }
    }
}
