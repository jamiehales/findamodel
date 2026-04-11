* Allow hulls to be calculated in a separate pass

* If there is an image in a directory, allow setting this as the collection preview, creator preview

* findamodel config files should have a model metadata dictionary field, these would always need to be alongside the model file itself, the key is the file path, and the object contains metadata for that model, such as preview image and name

* Improve preview render - use the same zoom/layout as the default view in threejs? - could threejs even be used from the command line (is there for example a chromium version that can render headless?)

* Add support for lychee (lys, lyt) and chitubox (ctb) files, but, if a lychee file is included in a printing list add a note to the printing layout page that says these files will not be included in the exported plates, and show an empty image for these on the model cards