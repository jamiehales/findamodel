FEATURE:
* Add support for referencing between supported/non-supported/full model variants - with rules configurable via findamodel.yaml
* When exporting a plate or downloading a model, allow downloading the model and supports separately
* Add SSGINode to threejs with webgpurenderer for better quality previews
* Add higher quality renders with GI and AO (through blender running headless?)
* Create an electron (or some other) wrapper for the app, so it can be run standalone on mac, windows and linux
* Support creating a secondary convex and concave hulls, which allows ignoring the first X layers of a model, effectively allowing rafts to be smooshed together

* Add support for "multiplates" by supporting several bins
  * An add plate button should add another canvas below the first
  * Right clicking on an object within the canvas moves it to the top of the plate with the most space
  * Purge all out of bounds button - moves everything out of bounds one by one to the top of the plate with the most space

* Allow freezing a model in place with middle click? Would be useful for placing large models where you need them with the correct rotation