use std::{
    collections::HashMap,
    env, fs, io,
    path::{Path, PathBuf, absolute},
};

use anyhow::{Context, Result, bail};
use clap::{ArgAction, CommandFactory, Error, Parser, crate_authors, error};
use image::{ImageBuffer, Rgb};
use resvg::usvg::{Options, Transform, Tree};
use tiny_skia::{ALPHA_U8_OPAQUE, Pixmap, PremultipliedColorU8};

const MAX_BACKGROUNDS: usize = 16;

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
        "Options:\n{options}"
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
        value_parser = parse_icon_width,
        help = "Width of each icon section in pixels (1-256)"
    )]
    icon_width: u32,
    #[arg(
        short = 'b',
        long = "background",
        action = ArgAction::Append,
        num_args = 1,
        value_name = "R,G,B",
        value_parser = parse_rgb,
        help = format!("Background color in RGB format, up to {MAX_BACKGROUNDS} times (e.g., 255,0,0)")
    )]
    backgrounds: Vec<Rgb<u8>>,
}

struct PreviewJob {
    svg: PathBuf,
    png_name: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Get the list of jobs to process based on the input files
    let preview_jobs = get_preview_jobs(&cli.input)?;

    let background_colors =
        get_background_colors(&cli.backgrounds).unwrap_or_else(|error| error.exit());

    // Create a temporary directory for storing preview images
    let temp_dir = tempfile::tempdir().context("Cannot create temporary directory")?;

    // Generate preview images for each input file and background color combination
    let output_dir = create_previews(
        &preview_jobs,
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
        preview_jobs.len(),
        if preview_jobs.len() == 1 { "" } else { "s" },
        normalize_display_path(&output_dir).display()
    );

    Ok(())
}

fn get_preview_jobs(inputs: &[PathBuf]) -> Result<Vec<PreviewJob>> {
    let mut filestems: HashMap<String, PathBuf> = HashMap::new();
    let mut preview_jobs = Vec::new();
    let mut add_file = |path: &PathBuf| -> Result<()> {
        let extension = get_file_extension(path)?;
        if !is_svg_extension(&extension) {
            bail!(
                "Unsupported file type: {}",
                normalize_display_path(path).display()
            );
        }

        let filestem = get_filestem(path)?;
        let key = filestem.to_lowercase();
        if let Some(existing) = filestems.get(&key) {
            eprintln!(
                "⚠ Skipping {} (same output name as {})",
                normalize_display_path(path).display(),
                normalize_display_path(existing).display()
            );
            return Ok(());
        }
        let absolute_path = absolute(path).with_context(|| {
            format!(
                "Cannot resolve path: {}",
                normalize_display_path(path).display()
            )
        })?;
        filestems.insert(key, path.clone());
        preview_jobs.push(PreviewJob {
            svg: absolute_path,
            png_name: format!("{filestem}.png"),
        });
        Ok(())
    };

    for input in inputs {
        let extension = get_file_extension(input)?;
        match extension.as_str() {
            ext if is_svg_extension(ext) => add_file(input)?,
            ext if ext.eq_ignore_ascii_case("txt") => {
                let base_dir = input.parent().unwrap_or_else(|| Path::new("."));
                let contents = fs::read_to_string(input).with_context(|| {
                    format!(
                        "Cannot read TXT file: {}",
                        normalize_display_path(input).display()
                    )
                })?;

                contents
                    .trim_start_matches('\u{feff}')
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .try_for_each(|line| add_file(&base_dir.join(line)))?;
            }
            _ => bail!(
                "Unsupported file type: {}",
                normalize_display_path(input).display()
            ),
        }
    }

    Ok(preview_jobs)
}

fn get_filestem(path: &Path) -> Result<String> {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .with_context(|| {
            format!(
                "Path has no filename: {}",
                normalize_display_path(path).display()
            )
        })
}

fn get_filename(path: &Path) -> Result<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .with_context(|| {
            format!(
                "Path has no filename: {}",
                normalize_display_path(path).display()
            )
        })
}

fn get_file_extension(path: &Path) -> Result<String> {
    path.extension()
        .map(|ext| ext.to_string_lossy().into_owned())
        .with_context(|| {
            format!(
                "Path has no extension: {}",
                normalize_display_path(path).display()
            )
        })
}

fn normalize_display_path(path: &Path) -> PathBuf {
    path.components().collect()
}

fn is_svg_extension(extension: &str) -> bool {
    extension.eq_ignore_ascii_case("svg") || extension.eq_ignore_ascii_case("svgz")
}

fn parse_icon_width(value: &str) -> Result<u32, String> {
    let width = value
        .parse::<u32>()
        .map_err(|_| "enter a whole number from 1 to 256")?;
    if !(1..=256).contains(&width) {
        return Err("width must be between 1 and 256 pixels".to_owned());
    }

    Ok(width)
}

fn parse_rgb(value: &str) -> Result<Rgb<u8>, String> {
    let mut components = value.split(',');
    let (Some(red), Some(green), Some(blue), None) = (
        components.next(),
        components.next(),
        components.next(),
        components.next(),
    ) else {
        return Err("expected exactly three comma-separated RGB values".to_owned());
    };

    let parse_component = |component: &str| {
        component
            .trim()
            .parse::<u8>()
            .map_err(|_| "RGB values must be integers from 0 to 255".to_owned())
    };
    let red = parse_component(red)?;
    let green = parse_component(green)?;
    let blue = parse_component(blue)?;

    Ok(Rgb([red, green, blue]))
}

fn format_rgb(Rgb([red, green, blue]): Rgb<u8>) -> String {
    format!("{red},{green},{blue}")
}

fn get_background_colors(backgrounds: &[Rgb<u8>]) -> Result<Vec<Rgb<u8>>, Error> {
    // Keep this check after parsing
    // so we check the number of background colors after parsing them
    if backgrounds.len() > MAX_BACKGROUNDS {
        let command = Cli::command();
        let literal = *command.get_styles().get_literal();
        return Err(Error::raw(
            error::ErrorKind::TooManyValues,
            format!(
                "too many values for '{literal}--background <R,G,B>{literal:#}': \
                 at most {MAX_BACKGROUNDS} background option occurrences are allowed\n\n\
                 For more information, try '{literal}--help{literal:#}'.\n"
            ),
        )
        .with_cmd(&command));
    }

    let default_colors = [Rgb([0, 0, 0]), Rgb([255, 255, 255])];
    let mut background_colors = default_colors.to_vec();

    for background in backgrounds {
        let skip_reason = if default_colors.contains(background) {
            Some("already included by default")
        } else if background_colors.contains(background) {
            Some("duplicate")
        } else {
            None
        };

        if let Some(reason) = skip_reason {
            eprintln!(
                "⚠ Skipping {reason} background color: {}",
                format_rgb(*background)
            );
            continue;
        }

        background_colors.push(*background);
    }

    Ok(background_colors)
}

fn create_previews(
    preview_jobs: &[PreviewJob],
    background_colors: &[Rgb<u8>],
    icon_width: u32,
    temp_dir: &Path,
) -> Result<()> {
    let mut opt = Options::default();
    opt.fontdb_mut().load_system_fonts();

    for preview_job in preview_jobs {
        // Loading and parsing the SVG file
        let svg_data = fs::read(&preview_job.svg).with_context(|| {
            format!(
                "Cannot read SVG file: {}",
                normalize_display_path(&preview_job.svg).display()
            )
        })?;

        opt.resources_dir = preview_job.svg.parent().map(Path::to_path_buf);

        let tree = Tree::from_data(&svg_data, &opt).with_context(|| {
            format!(
                "Cannot parse SVG file: {}",
                normalize_display_path(&preview_job.svg).display()
            )
        })?;

        // Calculate scaling factors
        // Icon widths are far below 2^23, so the conversion to f32 is lossless
        #[expect(clippy::cast_precision_loss)]
        let width = icon_width as f32;

        // Create Pixmap with target size
        let mut pixmap =
            Pixmap::new(icon_width, icon_width).context("Cannot create preview pixmap")?;

        let transform = fit_transform(tree.size().width(), tree.size().height(), width);

        // Render SVG with scaling
        resvg::render(&tree, transform, &mut pixmap.as_mut());

        let img_data = render_with_backgrounds(&pixmap, background_colors, icon_width)?;

        let temp_path = temp_dir.join(&preview_job.png_name);

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

fn render_with_backgrounds(
    pixmap: &Pixmap,
    background_colors: &[Rgb<u8>],
    icon_width: u32,
) -> Result<ImageBuffer<Rgb<u8>, Vec<u8>>> {
    let color_count =
        u32::try_from(background_colors.len()).context("Too many background colors")?;
    let combined_width = icon_width
        .checked_mul(color_count)
        .context("Combined preview width is too large")?;
    let mut combined = ImageBuffer::<Rgb<u8>, _>::new(combined_width, icon_width);

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

    Ok(combined)
}

// TECH DEBT: https://github.com/robertohuertasm/svg-icon-check/pull/3#discussion_r4659304463
fn blend(pixel: PremultipliedColorU8, background: Rgb<u8>) -> Rgb<u8> {
    let Rgb([bg_red, bg_green, bg_blue]) = background;
    let a = f32::from(pixel.alpha()) / f32::from(ALPHA_U8_OPAQUE);
    // With a in 0..=1, the result is always within 0..=255, so the cast cannot
    // truncate or lose the sign
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let blend_colors = |c: u8, bg: u8| (f32::from(c) + (f32::from(bg) * (1.0 - a))) as u8;
    Rgb([
        blend_colors(pixel.red(), bg_red),
        blend_colors(pixel.green(), bg_green),
        blend_colors(pixel.blue(), bg_blue),
    ])
}

fn finalize_output(temp_dir: &Path, output: PathBuf) -> Result<PathBuf> {
    let cwd = env::current_dir()?;
    let output_abs = match fs::canonicalize(&output) {
        Ok(path) => path,
        Err(_) => cwd.join(output),
    };

    // Create the output directory
    fs::create_dir_all(&output_abs).with_context(|| {
        format!(
            "Cannot create output directory: {}",
            normalize_display_path(&output_abs).display()
        )
    })?;

    let entries = fs::read_dir(temp_dir)
        .context("Cannot read temp directory")?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()
        .context("Cannot read temp directory entry")?;
    let backups = tempfile::tempdir().context("Cannot create backup directory")?;
    if let Err((error, rollback_failed)) = copy_previews(&entries, &output_abs, backups.path()) {
        if rollback_failed {
            let backup_path = backups.keep();
            return Err(error.context(format!(
                "Rollback failed; backups retained in {}",
                normalize_display_path(&backup_path).display()
            )));
        }
        return Err(error);
    }

    Ok(output_abs)
}

fn copy_previews(
    entries: &[PathBuf],
    output: &Path,
    backup_dir: &Path,
) -> std::result::Result<(), (anyhow::Error, bool)> {
    let mut copied: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let result = (|| -> Result<()> {
        for entry in entries {
            let filename = get_filename(entry)?;
            let target = output.join(&filename);
            let resolved = resolve_copy_target(&target)?;
            let backup = match fs::metadata(&resolved) {
                Ok(metadata) if metadata.is_file() => {
                    let backup = backup_dir.join(copied.len().to_string());
                    fs::copy(&resolved, &backup).with_context(|| {
                        format!(
                            "Cannot backup output file: {}",
                            normalize_display_path(&target).display()
                        )
                    })?;
                    Some(backup)
                }
                Ok(metadata) if metadata.is_dir() => {
                    fs::OpenOptions::new()
                        .write(true)
                        .open(&target)
                        .with_context(|| format!("Cannot copy file: {filename}"))?;
                    bail!("Cannot copy file: {filename}: destination is a directory");
                }
                Ok(_) => bail!("Cannot copy file: {filename}: destination is not a regular file"),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "Cannot inspect output file: {}",
                            normalize_display_path(&target).display()
                        )
                    });
                }
            };

            // Record the destination before copying: a failed copy can leave a partial file.
            copied.push((resolved, backup));

            fs::copy(entry, &target).with_context(|| format!("Cannot copy file: {filename}"))?;
        }
        Ok(())
    })();

    if let Err(error) = result {
        let mut rollback_errors = Vec::new();
        for (target, backup) in copied.into_iter().rev() {
            let result = match &backup {
                Some(backup) => fs::copy(backup, &target).map(|_| ()),
                None => match fs::remove_file(&target) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                    result => result,
                },
            };
            if let Err(rollback_error) = result {
                let backup_info = backup.map_or_else(String::new, |path| {
                    format!(" (backup: {})", normalize_display_path(&path).display())
                });
                rollback_errors.push(format!(
                    "{}: {rollback_error}{backup_info}",
                    normalize_display_path(&target).display()
                ));
            }
        }
        if rollback_errors.is_empty() {
            return Err((error, false));
        }
        return Err((
            error.context(format!("Rollback errors: {}", rollback_errors.join("; "))),
            true,
        ));
    }
    Ok(())
}

fn resolve_copy_target(target: &Path) -> Result<PathBuf> {
    let mut resolved = target.to_path_buf();
    for _ in 0..40 {
        match fs::symlink_metadata(&resolved) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let link = fs::read_link(&resolved).with_context(|| {
                    format!(
                        "Cannot read symlink: {}",
                        normalize_display_path(&resolved).display()
                    )
                })?;
                resolved = resolved
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(link);
            }
            Ok(_) => return Ok(resolved),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(resolved),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Cannot inspect output file: {}",
                        normalize_display_path(&resolved).display()
                    )
                });
            }
        }
    }
    bail!(
        "Too many symlinks resolving output file: {}",
        normalize_display_path(target).display()
    )
}
