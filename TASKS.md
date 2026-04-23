* Fix models before using for slicing/support calculation, etc... (See RUNEBRACE.md)
* The force generation seems incorrect when shown at the end - investigate how it's generated
* The forces being shown in the render viewport are in the wrong direction - they should be pulling towards the FEP, not crushing forces
* Stop the supports being pushed too high up the model - add a test to ensure that at each layer sliced all supports existing at that layer result in evenly distributed forces. If they do not, redistribute the positions of those supports. Supports that were created in another island are not eligable for movement
* Supports should be assigned their island ID - that is, the island they were originally created at.
* An island has an id that is a bitmap
* When islands join, that new island is given a new unique id based on the or of all parents ids (does that logic work?)
* Support (quarter/half/full resolution) slice preview on the plate page? Might be tricky, as we'd have to load in a large amount of mesh data

--

* By default use small tips, and increase them in size if the pull force gets too much and the support density vs the size of the contact face (based on the rough surface area to the normal of the contact point within a given - configurable - flatness) exceeds a configurable ratio
* Only use larger tips if the area which will be supported is relatively flat, and the support is a small portion of the contact area
* Implement optimizations

--

Now, iterate through all layers in support v3, and calculate force exerted on each support. Force is based on the total area of the top layer of that island. If the forces on all the supports supporting that island (taking into account the support tip size) exceed the strength of the resin, add a new support in the place that would reduce the force on the existing supports the most.

If there's more than 3mm (configurable) in the x/y direction from the last support, add a support int he location that is at the furthest distance from existing supports while not exceeding the distance require

* If forces are negative vertically, towards the build plate, that indicates need more of a support

* Take into account total weight of the current area
* Make sure overhangs are supported properly
* Add a final pass that moves the supports to the closest piece of the actual mesh

--

* Add note to AI to ignore orange parts of the image, or somehow filter them out before passing it over? Or render previews both with and without supports?

* Add the ability to duplicate a list, or copy models into another list? Something for better list item management (staging lists, combining plates, etc...)

* Add support for generating (and caching) hulls on the fly - when added to a printing list, or when the model page is opened. Put behind turning off a "Precalculate hulls" toggle?

* When rendering the item without supports, can we check if there's a rotation on the object and if there is reset it, so that it's "upright"?

* The -/+ buttons on a model card that add and remove the model from the printing list, make those buttons a single text "add to printing list" button when quantity is 0, when the quantity is >=1 keep the remaining +/- buttons

* Bug: Removing a model from the print plate while on the printing list page reloads the entire page

* It looks like the layout might not be stored in the database, but in the local browser?
* Add support for building desktop as portable installation - not just installer

---

Now implement all the methods you suggested, as pngzip_<method>, making sure to hook them up in the frontend. Design a test harness that runs tests for the following model types, where in it generates the model, then validates that the image output from the method is correct. These are the model types - a sphere, a 3 sided pyramid, a cube, and a cylinder. Write a test case to test the image output (raw code backed bitmap, not encoded to png, jpg etc...). The test case should validate via calculated mathematics expectation of the shape at any given layer, not using the algorithms designed to work on polygonal data that we're testing. Spin up subagents for each of these, implementing each in a separate service (or at least a separate class coordinated by the same service). In each subagent implement the slicing algorithm, then validate first on the cube, adding then the cylinder, then the sphere and finally the pyramid until all 4 work. Fix up and iterate on the algorithms based on the results of each test. In the application the implementation should be swappable at compile time via an enum in order to set which implementation is used when clicking download zip.

* See about optimizing triangle intersection by using an octree or just an indexed list of layer overlaps or similar for reducing queries?
* Add progress bar for slice generation when downloading
* Multithread the slicing algorithm for speed
* Add support for anti-aliasing
* Add support for anti-aliasing only on supports
* Expose private const int DefaultLayerBatchSize = 8; in settings (and document)
* Document DefaultRowGroupHeight and GenericComputeWorkgroupSize and maybe expose them
* Add support for ctb for uniformation
* Add support for overexposing supports (or just support tips? add rerf test support for it?)
* Move slice to it's own button, and use gpu by default only falling back to cpu if supported gpu is unavailable
* Slicing progress is wrong, it shows "slice layers 9-16 of 1109" but "generating slices 0 of 1109"
* Slice CPU/GPU usage is very low, and slicing is incredibly slow

--

Add an auto supporting algorithm, calculating the pull forces at each layer as it calculates the mesh, determining where an additional support may be needed.

Add an autosupports configuration preview on the autosupports settings screen - when the values are changed allow a button to be pressed that regenerates and shows the generated supports on the following shapes - each in it's own viewport
* A thin plane (2mm) of 40x40mm parallel to the build plate
* A thin plane (2mm) of 40x40mm at a 30 degree angle offset from the build plate
* A sphere of diameter 40mm
* A cube of diameter 40mm
* A cube of diameter 40mm rotated by 45 degrees
* An upside down cone (point facing downwards) of diameter 40mm and height 80mm
* Support for some hardcoded stls, not yet provided - implement a list that can be populated (in code for now), and will automatically add new viewports and support generation for each of them

* Remove the v3 from all autosupport settings, this is now the only algorithm
* Add support for items 6-10 in the support generation algorithm document (SUPPORT-ALGORITHM-RESEARCH.md), adding settings to the model and settings page where recommended, as well as docs in the docs page
* Multithread the autosupport where possible

* When finalizing support positions, place the support tip at the closest x,y,z position which meets the mesh within the voxel bounds, at the lowermost y position
* Add the reason the support was generated when you click on a support tip in the autosupport window - e.g. large island, fixing pull force/crush force, etc... also add a toggle to render a number by each support based on the order they were created
* Remove unused autosupport settings from config
* Add support to toggle between rendering the real mesh and the voxel mesh in autosupports

Next:
* Add support trunk generation - support on a grid basis (no model overlap)
* Add scaffolding generation (no model overlap)

To calculate pull forces, research your own algorithms to determine the best, but take into account this one:
* Voxelize the mesh (at a lower resolution than the printer) and calculate the forces on each support using it

Follow this thought process for the support generation:
* Slice until the first layer where there is mesh
* When mesh is discovered, place a support at the center point of mass of each island of pixels
* Slice until the pull forces on that support exceed a fixed threshold (to be determined). Note that pull forces can be 3 dimensional
* Add another support at a location that will reduce the forces the most (this can be approximate if hard to compute) - note that supports can be generated at the edge of the mesh, not just at the center, keep into account the best locations based on the reduction in forces
* Keep adding supports until the fixed threshold for pull forces is no longer exceeded
* Slice more layers until there is a pull force that exceeds the tolerance
* If there is a new island on any new slice layer, add a support at the center of mass of that location before continuing

--


* Add a function that can use an existing supported model to detect support points, and modify heuristics to generate supports in a similar manner
* Generate different pull forces based on support contact size. Differ contact size based on point on model and requirements from overall supported weight? Weight towards larger supports at the base of the model
