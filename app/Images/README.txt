HED SPACE — STANDARD APPLICATION HERO IMAGES

`hero-images.json` can point each section to EITHER one image OR several images.

ONE IMAGE:
  "library": "library.jpg"

SEVERAL IMAGES:
  "library": [
    "library-01.jpg",
    "library-02.jpg",
    "library-03.jpg"
  ]

When several images are configured, HED Space:
  1. checks which files actually exist,
  2. randomly chooses ONE valid image when the app launches,
  3. keeps that image stable for the whole session,
  4. chooses again on the next app launch.

All configured files must be inside this exact `app/Images/` directory.

Supported formats:
  JPG / JPEG / PNG / WEBP

Recommended:
  3840 x 2160 preferred
  1920 x 1080 minimum

Example:
{
  "home": ["home-01.jpg", "home-02.jpg", "home-03.jpg", "home-04.jpg"],
  "watch": ["watch-01.jpg", "watch-02.jpg"],
  "library": ["library-01.jpg", "library-02.jpg"],
  "listen": ["listen-01.jpg"],
  "atmospheres": ["atmosphere-01.jpg", "atmosphere-02.jpg"]
}

If NONE of the configured files for a section exist, HED Space falls back to
dynamic media-library artwork for that section.

After editing hero-images.json or adding/replacing images:
  stop the app with Ctrl+C
  run: npm run dev

You do NOT need to run npm install for image/config-only changes.
