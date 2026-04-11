* Use a library for DTO generation?

* Add unit tests for services

* Allow hulls to be calculated in a separate pass

* Add a 'download all models' button to the printing list page which zips up all the model files selected, and downloads it - show a progress spinner while the zipping is happening so the user is informed. Show progress of the zip process if possible.

* If there is an image in a directory, allow setting this as the collection preview, creator preview

* findamodel config files should have a model metadata dictionary field, these would always need to be alongside the model file itself, the key is the file path, and the object contains metadata for that model, such as preview image and name

* Determine if db.Database.ExecuteSqlRaw("PRAGMA journal_mode=WAL;"); is needed

* On the main screen show the X most recently added models (configurable via config variable)

* Add warning if there are any models off canvas when hitting save plate

* Add option to show or hide labels

* Right align the reset button

* Make the hardcoded 2mm raft clipping configurable - it should default to 2mm, but be overridable using the findamodel.yaml metadata

* Add 3D Print type to model, FDM / Resin / Any

* UpdateLogger so the service/context is passed in separately from a constant as a channel, so that certain channels can be enabled or disabled, investigate logging libraries before choosing a method here
* By default any logs of the ExecutedDbCommand should be set to Debug level, and that logging channel disabled
* See if there's a VSCode serilog plugin or similar that can provide the ability to filter channels dynamically

* Potentially investigate some sort of model simplification algorithm?

* Determine the feasibility of rendering supports transparently

* Improve preview render - use the same zoom/layout as the default view in threejs?

* Support related parts + related models (same collection?) when viewing a model's detail page