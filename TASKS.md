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

* Finish brace export in model VOXLs (`backend/src/geometry/support_layout.rs`). Braces between traced supports are exported, but two kinds are still dropped: horizontal bars fused into the same mesh as the posts they join (their cross-sections merge with the posts, so no walk leaves them untraced), and braces with an end on a post that was never traced (e.g. the ARCS bracket's two missing posts). Check in DragonFruit that the exported braces load onto the right knots

* Finish kickstand export in model VOXLs (`backend/src/geometry/support_layout.rs`). Legs splaying from a traced trunk to the plate, and posts with a strut leaning off their top onto a trunk, are exported as DragonFruit kickstands, but on Eiren's Wednesday about a sixth of the leaning legs near the plate are still left out: the legs splaying from those posts (a DragonFruit kickstand cannot hold up another kickstand, so they need a decision on how to export them) and struts leaving a post below its top. Some trunks there that stand only on legs are also traced down one leg and across the X where it crosses a neighbouring leg, so their root lands between two feet. The kickstands load through DragonFruit's own VOXL import code but have not been checked in the app

* The walk below a steeply leaning tip is sparse (`backend/src/geometry/support_layout.rs`): until it has a direction it steers as if the shaft were upright, so it skips the tip and only picks the shaft up again by giving up after 1.5 mm, then keeps doing so while its direction still comes from the tip's first slices. The tip is fitted to the slices it skipped, but the shaft below gets a point every 1.5 mm or so, and the blob it gives up on can sit off the shaft: a stray joint on a few supports (Eiren's Wednesday #63 and #66, the Commander's #3), and a tip running into a joint ball takes its socket from that blob, up to 2 mm past the ball (ARCS tips are 1.24x the original cones' length at the median). Restarting the walk's direction where it picks the shaft up again filled the shafts in but added joints and re-parented supports across the sample models, so re-validate joints and the trees

* Try rebuilding support layouts by fitting the support mesh's own shapes instead of slicing (`backend/src/geometry/support_layout.rs`). Many support exports build supports from separate closed shapes that only overlap (each rod, tip cone and joint ball its own shell): 4 of the 6 sample models are built this way, while Eiren's Wednesday and most of Crew 2 are fused into one mesh. Fitting each shell gives its exact axis and end radii at any angle, which covers slanted tips, braces, kickstand legs and struts without the horizontal-slice heuristics. First count how common shape-built supports are across the pre-supported models on the findamodel instance. If they are common, build a trial that fits the shells, joins them into supports where they meet (mostly at joint balls), and falls back to slicing for fused geometry, then compare it with the slicing rebuild on the same probe metrics (tips matched, joints, surface coverage, contact and shaft diameters)

* Optimization: writing a model VOXL takes 0.8 to 1 s on the larger sample models (Balin, Crew 2, Eiren's Wednesday; 200k to 560k triangles), the slowest stage of the download. Nearly all of it is compressing the model mesh chunk on one core at the default zlib level (`backend/src/geometry/voxl.rs`). Try a faster compression level or compressing in parallel, and weigh the time saved against the larger file
