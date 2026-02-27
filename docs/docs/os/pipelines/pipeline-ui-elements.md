---
title: Pipeline UI JSON
---

This document describes the JSON structure used by the Pipeline UI system. The UI definition lives on the pipeline graph metadata under `helios.pipeline.ui`.

## Top-level shape

```json
{
  "title": "Pipeline UI",
  "description": "Curated tuning controls.",
  "layout": {
    "type": "tabs",
    "tabs": [
      {
        "id": "main",
        "title": "Main",
        "content": []
      }
    ]
  }
}
```

Fields:

- `title` (string): Display title for the UI.
- `description` (string, optional): Short description.
- `layout` (object, optional): Either a `tabs` layout or a `stack` layout.
- `groups` (array, legacy optional): Older grouping format; prefer `layout`.

## Layout containers

### Tabs

```json
{
  "type": "tabs",
  "tabs": [
    {
      "id": "preprocess",
      "title": "Preprocess",
      "content": [
        { "type": "text", "id": "preprocess_intro", "text": "Prepare the frame." }
      ]
    }
  ]
}
```

### Stack

```json
{
  "type": "stack",
  "items": [
    { "type": "title", "id": "stack_title", "text": "Controls" }
  ]
}
```

### Group

```json
{
  "type": "group",
  "id": "group_mask",
  "title": "Mask",
  "description": "HSV mask settings.",
  "items": [
    { "type": "text", "id": "mask_intro", "text": "Define the range." }
  ]
}
```

### Accordion

```json
{
  "type": "accordion",
  "id": "advanced_mask",
  "title": "Advanced",
  "description": "Optional tweaks",
  "defaultOpen": false,
  "items": []
}
```

## Text elements

### Title

```json
{ "type": "title", "id": "section_title", "text": "Section" }
```

### Text

```json
{ "type": "text", "id": "intro_text", "text": "Helpful description." }
```

### Divider

```json
{ "type": "divider", "id": "divider_1", "label": "Optional label" }
```

## Controls

All controls support:

- `id` (string): Unique key used for editing and persistence.
- `label` (string): Display label.
- `help` (string, optional): Short helper text.
- `bind` (string or object, optional): Maps control to a node input. See binding examples below.

### Slider

```json
{
  "type": "slider",
  "id": "blur_sigma",
  "label": "Blur sigma",
  "bind": "2.sigma",
  "min": 0,
  "max": 10,
  "step": 0.1,
  "default": 0.8,
  "trackGradient": "linear-gradient(90deg, #0ea5e9, #22c55e)"
}
```

### Dual slider (min/max)

```json
{
  "type": "dual_slider",
  "id": "area_range",
  "label": "Area range",
  "bind": {
    "min": "7.min_area",
    "max": "7.max_area"
  },
  "min": 0,
  "max": 200000,
  "step": 10,
  "default": [500, 50000]
}
```

### Toggle

```json
{
  "type": "toggle",
  "id": "enable_filter",
  "label": "Enable filter",
  "bind": "7.enabled",
  "default": true
}
```

### Select

```json
{
  "type": "select",
  "id": "blur_mode",
  "label": "Blur mode",
  "bind": "2.mode",
  "options": ["cpu", "gpu"],
  "default": "cpu"
}
```

If `options` is omitted, the UI will try to use enum metadata from the bound port when available.

### Input

```json
{
  "type": "input",
  "id": "custom_label",
  "label": "Custom label",
  "bind": "9.label",
  "default": "Target"
}
```

### Color (pixel input)

```json
{
  "type": "color",
  "id": "overlay_color",
  "label": "Overlay color",
  "bind": "10.color",
  "default": "#22c55e"
}
```

### HSV target

```json
{
  "type": "hsv",
  "id": "target_hsv",
  "label": "HSV Target",
  "hsvDefaults": { "h": 120, "s": 0.7, "v": 0.8 },
  "bind": {
    "h": "5.h",
    "s": "5.s",
    "v": "5.v"
  }
}
```

You can also use a shorthand string binding (`\"bind\": \"5\"` or `\"bind\": \"5.h\"`) to target ports named `h`, `s`, and `v` on the same node.

### HSV range

```json
{
  "type": "hsv_range",
  "id": "mask_hsv_range",
  "label": "HSV Range",
  "hsvRangeMode": "include",
  "hsvRangeDefaults": {
    "h": [50, 160],
    "s": [0.25, 1],
    "v": [0.2, 1]
  },
  "bind": {
    "h": { "min": "3.h_min", "max": "3.h_max" },
    "s": { "min": "3.s_min", "max": "3.s_max" },
    "v": { "min": "3.v_min", "max": "3.v_max" }
  }
}
```

You can also use a shorthand string binding (`\"bind\": \"3\"` or `\"bind\": \"3.h_min\"`) to target ports named `h_min`, `h_max`, `s_min`, `s_max`, `v_min`, and `v_max` on the same node.

### Layout toggle

```json
{
  "type": "layout_toggle",
  "id": "layout_2x2",
  "label": "Enable 2x2 layout",
  "default": false,
  "layout": {
    "rows": 2,
    "columns": 2,
    "outputKeys": {
      "0:0": "main",
      "0:1": "aux",
      "1:0": null,
      "1:1": null
    }
  }
}
```

## Binding rules

- Binding strings use `nodeId.portKey` (e.g. `3.h_min`).
- `nodeId` can be the graph node id, backend id, or source id; the UI resolves the first match.
- If `bind` is omitted, the control uses local UI state only and does not change pipeline values.

## Embedding in a pipeline template

```json
{
  "metadata": {
    "helios.pipeline.ui": {
      "title": "HSV Color Detection",
      "description": "Curated controls for HSV masking.",
      "layout": {
        "type": "tabs",
        "tabs": [
          {
            "id": "mask",
            "title": "Mask",
            "content": [
              { "type": "text", "id": "mask_intro", "text": "Define the HSV range." },
              {
                "type": "hsv_range",
                "id": "mask_hsv_range",
                "label": "HSV Range",
                "hsvRangeDefaults": { "h": [50, 160], "s": [0.25, 1], "v": [0.2, 1] },
                "bind": {
                  "h": { "min": "3.h_min", "max": "3.h_max" },
                  "s": { "min": "3.s_min", "max": "3.s_max" },
                  "v": { "min": "3.v_min", "max": "3.v_max" }
                }
              }
            ]
          }
        ]
      }
    }
  }
}
```
