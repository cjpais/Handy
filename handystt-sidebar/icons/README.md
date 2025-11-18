# HandySTT Sidebar Icons

This directory should contain the extension icons in PNG format:

- **icon16.png** - 16x16px (toolbar icon)
- **icon48.png** - 48x48px (extension management)
- **icon128.png** - 128x128px (Chrome Web Store)

## Creating Icons

You can create icons using the existing HandySTT branding or create new ones.

### Recommended Design

- Use a microphone symbol 🎤
- Brand colors: Purple/blue (#667eea) on dark background
- Simple, recognizable design
- High contrast for visibility

### Using Online Tools

1. Visit https://www.favicon-generator.org/
2. Upload a source image (SVG or PNG)
3. Generate multiple sizes
4. Download and place in this directory

### Using ImageMagick (Command Line)

```bash
# Convert from SVG
convert -background transparent -density 300 icon.svg -resize 16x16 icon16.png
convert -background transparent -density 300 icon.svg -resize 48x48 icon48.png
convert -background transparent -density 300 icon.svg -resize 128x128 icon128.png
```

## Temporary Placeholders

For development, you can use simple colored squares:

```bash
convert -size 16x16 xc:'#667eea' icon16.png
convert -size 48x48 xc:'#667eea' icon48.png
convert -size 128x128 xc:'#667eea' icon128.png
```
