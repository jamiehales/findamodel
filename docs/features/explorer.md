---
layout: default
title: Explorer
parent: Features
nav_order: 1
---

# Explorer

The Explorer page is the main entry point for browsing your model library by directory structure.

## Navigation

The left panel shows a tree of your model root directory. Click a folder to navigate into it. Folders that contain `findamodel.yaml` files show a configuration icon.

## Folder cards

Each subfolder is displayed as a card showing:
- Folder name
- Number of model files inside (recursively)
- An **Edit** button to open the configuration editor for that folder

## Model cards

Model files in the current directory are displayed as cards showing:
- Preview image (generated at index time)
- Model name (resolved metadata)
- Creator and collection labels
- A badge if the model has supports

Click a model card to open the **Model detail page**.

## Editing directory configuration

Click **Edit** on a folder card to open the configuration editor. This lets you set or override metadata fields and rules for that directory. Changes are written directly to the `findamodel.yaml` file and the affected directory tree is re-resolved immediately.

## Indexing

An **Index** button appears next to the edit button. Clicking it queues a directory scan for that folder, refreshing any metadata or hull changes.

The **indexer status indicator** in the toolbar shows when indexing is in progress and how many items remain in the queue.
