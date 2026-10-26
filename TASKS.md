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

* Finish kickstand export in model VOXLs (`backend/src/geometry/support_layout.rs`). Legs splaying from an untraced post to the plate are still left out (most of the 44 of 303 leaning leg sections near the plate on Eiren's Wednesday that no kickstand covers): the post is a DragonFruit kickstand, which cannot hold up another kickstand, and a brace cannot end below the root top. DragonFruit's trunk does not need a contact cone, so such posts could be rebuilt as trunks without one, holding their legs as kickstands; decide whether that is wanted

* Finish tracing shafts that lean into a neighbour (`backend/src/geometry/support_layout.rs`). The walk now takes up the shaft below a steeply leaning tip as soon as its slices settle into a straight run, and passes up ways beside the shaft (another support's slices touching it), but where a leaning shaft converges on an upright post whose middle lies within the shaft's own box, the walk still takes the post at the first fork, with a sideways step where it switches (Eiren's Wednesday #66). Where a post ends in a V of two legs (Blighttown), which leg it follows is a tie-break, so small changes to the walk swap branches between trees. A tapered trunk (BS Sculpt, Eastman's posts) gets one shaft diameter, a third of the way up its slices' widths, so its foot reads thin

* Optimization: writing a model VOXL takes 0.8 to 1 s on the larger sample models (Balin, Crew 2, Eiren's Wednesday; 200k to 560k triangles), the slowest stage of the download. Nearly all of it is compressing the model mesh chunk on one core at the default zlib level (`backend/src/geometry/voxl.rs`). Try a faster compression level or compressing in parallel, and weigh the time saved against the larger file

* A separate part of a model without supports can still pass as a support when it sits under the part it touches (`backend/src/geometry/support_layout.rs`): on Mezgike's Runt Kanz blaster ammo (marked supported, in an Unsupported folder) the casing is a shell of its own under the bullet and rebuilds as one support holding it up. Parts classed as supports now pass only when their rebuilt supports cover 70% of their surface and at least a quarter of those 1 mm or longer hold the model up (the tip meets a downward-facing surface from outside), which leaves none of the 739 sampled Engin3d Wartrains models or the other mis-marked sculpts seen so far with supports. A minimum support count would not catch the ammo: genuine pre-supported bits with two or three supports exist. The genuine models closest to the bar are parts held mostly by supports leaning on their sides (Loot Studios' Crowchaser Golem cable: 56% of its supports hold it up)

* Small solid pieces of a sculpt lying apart from the model are classed as support (`backend/src/geometry/support_separation.rs`), such as a 0.6 mm² pebble beside Death x Tiles' FST_032 Hollow A, which rebuilds as a stub strut. Slivers and voids are kept with the model, but a closed piece with volume outside the model looks like a support twig, and repaired meshes (Creature Caster's Alice bust) have hundreds of small unrepeated pieces whose origin is unclear. It matters little: a piece that small is barely visible in the supports-removed preview

* Fix what building the exported VOXLs with DragonFruit's own code shows the rebuild gets wrong (`backend/src/geometry/support_layout.rs`). DragonFruit now builds what we export as meant (no knots moved on opening, braces at their own diameter, contacts as wide as the originals where they leave the model); what is left are rebuild problems. On Bite the Bullet's hub-style supports (Cultist Leader, Boss Dragon Tail) a fan of tips is fitted as cones about 8 mm long meeting in one ball, so the braces between those tip cones have no shaft to hang from and are left out. On Aradia's and the calibration boxes' small parts, "struts" whose two contacts are 0.1 to 1 mm apart with normals facing the same way are model pegs, not supports. On BS Sculpt's Balin two touching parallel posts rebuild as one. About 5,600 mm² of original support surface across the 154-model check corpus has nothing rebuilt within 1.5 mm of it, most of it on Eastman's busts and Bite the Bullet's hubs

* Rebuild the remaining braces and posts on Eastman busts (`backend/src/geometry/support_layout.rs`). Eastman's busts and bases rebuild at 91 to 99% support surface coverage. The lowest brace of each zigzag between neighbouring posts, whose lower end sits at or below the root top, is still dropped, since a DragonFruit brace needs both ends on a shaft above the root top and moving the end up to the root top would take the brace out of the original. Where the two braces of an X between the posts inside the hollow meet each post in one junction, they come out as one level bar: splitting each junction into its lower and upper part found the Xs, but also split single diagonals between posts about 2 mm apart into Xs and lowered round-trip precision. On Magneto's heads most tall posts under fan-shaped clusters of branches now come out as kickstands under the shaft they run into, but a few are still left out; on Magneto Head5 the missing post seems to have no tip of its own (its top is where the fan's branches meet), so no walk starts on it. Eastman's flat base plate (bars 0.5 mm thick lying on the plate between the roots) is a raft, DragonFruit's Line raft, which DragonFruit builds from its own raft setting and does not store in VOXL files, so it stays out of the rebuild

* Finish rebuilding struts inside hollowed models (`backend/src/geometry/support_layout.rs`). On Death x Tiles' hollowed tiles every support is a strut from the model to the model inside the hollow, tied by crossbar rungs. Most tiles now rebuild at 96 to 100% support surface coverage (Mineral Wall A 99%) with each strut in one piece on its own feet and its rungs as braces; what is left: on the Diagonal Cage (92%) struts that meet in pairs at one contact on the ceiling come out as one (the second is taken for a second top of the first one's tip), and three struts packed into one outline come out as two; on the Working Table one strut of a cluster of three still joins its neighbour where a rung merges them. Strut feet come out at about 0.6x the length of the original foot cones (tipgt_probe's lower-tip line) and heads at about 0.8x. When validating, count both contacts of a strut as tips
