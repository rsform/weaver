# Theme and Colour Scheme Redesign

**Date:** 2025-11-03
**Status:** Design approved, ready for implementation

## Overview

Redesign the theme lexicon to support expressive colour schemes comparable to base16 and Rose Pine, while maintaining flexibility for both preset themes and power user customization.

## Requirements

- Support 16+ semantic colour slots (comparable to base16 expressiveness)
- Enable importing/adapting popular themes (base16, Rose Pine, etc.)
- Separate light/dark modes as distinct themes (users can mix and match)
- Semantic naming over positional identifiers (readable, consistent across themes)
- Support preset picker UI + power user manual customization

## Design

### Two-Lexicon Structure

Split colour schemes from themes to enable reusability and mixing:

**1. `sh.weaver.notebook.colourScheme`** - Standalone colour palette record
- Standalone AT Protocol record
- Contains name, variant (dark/light), and 16 colour slots
- Can be published and referenced by multiple themes
- Enables sharing palettes between users

**2. `sh.weaver.notebook.theme`** - Complete theme with colour references
- References two colourScheme records (dark and light) via strongRefs
- Contains fonts, spacing, codeTheme (unchanged from previous design)
- Users can point to any published colour schemes, including others' palettes

### 16 Semantic Colour Slots

Organized into 5 semantic categories with consistent naming:

**Backgrounds (3):**
- `base` - Primary background for page/frame
- `surface` - Secondary background for panels/cards
- `overlay` - Tertiary background for popovers/dialogs

**Text (1):**
- `text` - Primary readable text colour (baseline)

**Foreground variations (3):**
- `muted` - De-emphasized text (disabled, metadata)
- `subtle` - Medium emphasis text (comments, labels)
- `emphasis` - Emphasized text (bold, important)

**Accents (3):**
- `primary` - Primary brand/accent colour
- `secondary` - Secondary accent colour
- `tertiary` - Tertiary accent colour

**Status (3):**
- `error` - Error state colour
- `warning` - Warning state colour
- `success` - Success state colour

**Role (3):**
- `border` - Border/divider colour
- `link` - Hyperlink colour
- `highlight` - Selection/highlight colour

### Mapping to Existing Schemes

**Base16 compatibility:**
- Backgrounds map to base00-base02
- Foregrounds map to base03-base07
- Accents/status/roles map to base08-base0F

**Rose Pine compatibility:**
- Direct semantic mapping (base→base, surface→surface, overlay→overlay)
- text/muted/subtle map to text/muted/subtle
- Accent colours map to love/gold/rose/pine/foam/iris

## Implementation Impact

### Files to Create
- `lexicons/notebook/colourScheme.json` ✓ (created)

### Files to Modify
- `lexicons/notebook/theme.json` ✓ (modified)
- Run `./lexicon-codegen.sh` to regenerate Rust types
- Update `crates/weaver-common/src/lexicons/mod.rs` (remove duplicate `mod sh;`)

### Code Changes Needed
- Update theme rendering code to use strongRef lookups
- Update CSS generation to use new 16-slot colour names
- Create default colour schemes (at least one dark, one light)
- Update any existing theme records/configs to new format

### Future Enhancements
- Theme picker UI with preset browser
- Colour scheme validation/linting
- Auto-generate light from dark (and vice versa) where appropriate
- Import converters for base16 YAML, Rose Pine JSON
