* Do a whole debug pass on tags, I think it needs some work
=> Separate AI tag indexing from normal index, allow it to run separately (including separate cron/startup indexing)
=> Allow clicking on either standard or ai tags to add that to the filtere (note: ADD to - not replace)

* Investigate how expensive downscaling meshes is - some of the meshes are 60mb plus, which takes a while streaming to the frontend when on wifi. Evaluate whether minimizing that would speed that up, or if the compute cost would take longer than over the wire. Assume a it will be used mostly on a 1gbit lan connection, but will also occasionally be used on a 20mbit internet connection, having support for both is a valid option.

* Indexing seems to take a long time to actually start processing files... 30s or so for 8 files, no idea how long for 2000 files
=> If ai generation is happening, it almost looks as if it's doing all of that before starting anything else?

* Add a toggle to settings for enable description generation

* Add the ability to duplicate a list, or copy models into another list? Something for better list item management (staging lists, combining plates, etc...)

* Add support for generating (and caching) hulls on the fly - when added to a printing list, or when the model page is opened. Put behind turning off a "Precalculate hulls" toggle?

* When rendering the item without supports, can we check if there's a rotation on the object and if there is reset it, so that it's "upright"?

* Bug: Removing a model from the print plate while on the printing list page reloads the entire page

* It looks like the layout might not be stored in the database, but in the local browser?

* Add support for building desktop as portable installation - not just installer

* Change the save layout/reset buttons to play/pause/rewind

* Bug: When choosing "without raft / convex hull", it should only affect supported models, not unsupported

* When the concave hull saves less than a set amount of area over the convex hull (default 10%), use the convex hull instead. Show this with a small X icon in the top right corner of the concave hull image, with the percentage of area remaining

* Optimization: filter options with a search term take ~0.75s on a 60k catalog (each facet runs a LIKE scan over the facets index). Make them scale to 100k+ models, e.g. full-text search or sharing one filtered pass across facets

* Replace the pull to edges toggle with an icon toggle that cycles between pull to edges, pull to center and pull to bottom (with matching layout behaviour). Icons: edges is a square outline with dots along all four inner edges, center is a square outline with a cluster of dots in the middle, bottom is a square outline with a row of dots along the bottom edge

* Cache generated model VOXLs (`/api/models/{id}/voxl`, support layout rebuilt from the detected supports) on disk, keyed by the model checksum, raft height and support layout version, up to a configurable max cache size. When the cache is full, evict the entries with the oldest last-accessed date

* Export braces in model VOXLs: braces between support posts are currently dropped from the rebuilt support layout (`backend/src/geometry/support_layout.rs`). Write them as DragonFruit braces (two knots on the posts they join plus a diameter). Covers diagonal braces (untraced chains of cross-sections between two traced supports), horizontal bars (wide cross-sections merged with the posts they join, e.g. the bar tying a bracket's tips together), and not mistaking the raft, joint hubs or crossing shafts for braces. Verify DragonFruit loads the current trunk/branch/twig/stick output first, since braces hang off knots on those segments

* Rebuilt supports whose tip leans steeply from the joint it comes off get a tip cone that is far too short (`backend/src/geometry/support_layout.rs`): the walk down from the contact has no direction yet, skips the first 1.5 mm or so of a slanted shaft as drifting, and is left with too few slices to fit the tip. On the Bite the Bullet Commander floor tile, 3 long slanted tips (about 0.3 to 0.5 mm over 3.3 mm) come out with their socket 0.1 mm from the contact. Contact positions and diameters are right. An earlier attempt to estimate the direction from the first few slices added joints across the sample models, so re-validate joints and coverage
