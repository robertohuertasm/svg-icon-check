# SVG Icon Check

Small utility to check how an SVG icon looks like on different backgrounds.

The utility has been created to help with the development of the [vscode-icons](https://github.com/vscode-icons/vscode-icons) project.

## Usage

The only required argument is the path to the SVG icon file.

```bash
svg-icon-check <icon.svg> 
```
![01](docs/example1.png)

By default, the icon is displayed both on a white and a black background. The background color can be changed using the `-b` option.

```bash
svg-icon-check <icon.svg> -b 245,245,220
```

![02](docs/example2.png)

You can define more than one background color by providing a space-separated list of RGB values.

Here we are using all the RGB values from the VS Code theme background colors section.

```bash
svg-icon-check <icon.svg> -b 30,30,30 40,42,54 46,52,64 47,54,59 39,40,34 60,63,65 88,110,117 89,91,79 120,120,120 253,246,227 236,239,244 242,242,242 250,250,250 
```
![03](docs/example3.png)

# VS Code Theme Background Colors

## Dark Mode Colors
| Name | RGB | Hex |
|------|------|------|
| **Obsidian** (Very dark gray) | (30, 30, 30) | `#1E1E1E` |
| **Dracula Dark** (Purple-tinted dark) | (40, 42, 54) | `#282A36` |
| **Nord Dark** (Cool gray) | (46, 52, 64) | `#2E3440` |
| **Solarized Dark** (Muted brown-gray) | (47, 54, 59) | `#2F363B` |
| **Monokai Dark** (Warm deep gray) | (39, 40, 34) | `#272822` |

## Medium Contrast Colors
| Name | RGB | Hex |
|------|------|------|
| **Dim Gray** (Soft gray) | (60, 63, 65) | `#3C3F41` |
| **Solarized Base** (Dark teal-gray) | (88, 110, 117) | `#586E75` |
| **Monokai Medium** (Olive-brown) | (89, 91, 79) | `#595B4F` |
| **VS Code Default Gray** | (120, 120, 120) | `#787878` |

## Light Mode Colors
| Name | RGB | Hex |
|------|------|------|
| **Solarized Light** (Pale yellow) | (253, 246, 227) | `#FDF6E3` |
| **Nord Light** (Icy blue-white) | (236, 239, 244) | `#ECEFF4` |
| **VS Code Light+** (Soft beige-white) | (242, 242, 242) | `#F2F2F2` |
| **Almost White** (Warm light gray) | (250, 250, 250) | `#FAFAFA` |



