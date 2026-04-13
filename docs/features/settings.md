---
layout: default
title: Settings
parent: Features
nav_order: 4
---

# Settings

The Settings page controls global application behaviour.

## Default raft height

**Default raft height (mm)** sets the global cutoff used when computing sans-raft hulls for printing plate placement. Models are clipped at this height from the base before the hull footprint is calculated, so the raft or brim does not inflate the footprint.

This value can be overridden per-directory using the `raftHeight` key in `findamodel.yaml`.

## Theme

Choose between the **default** light/dark theme and the **Nord** colour scheme.

## Metadata dictionary

The metadata dictionary is an optional list of known values for `creator` and `collection` fields. When defined, these values appear as autocomplete suggestions in metadata editing dialogs. Dictionary values do not restrict what can be stored - they are hints only.

Add, edit, or remove dictionary entries from the Settings page.
