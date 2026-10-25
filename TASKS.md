* Do a whole debug pass on tags, I think it needs some work
=> Separate AI tag indexing from normal index, allow it to run separately (including separate cron/startup indexing)
=> Allow clicking on either standard or ai tags to add that to the filtere (note: ADD to - not replace)

* Some support parts aren't detected as supports - these are usually the tips of the supports

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
