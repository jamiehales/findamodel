* Add database migration support

* Switch 3D plate generation to 3mf with instancing

* Allow hulls to be calculated in a separate pass

* Expand the main page to fill the whole width of the screen

* Add a 'download all models' button to the printing list page which zips up all the model files selected, and downloads it - show a progress spinner while the zipping is happening so the user is informed. Show progress of the zip process if possible.

* Reduce complexity of convex hull
* Fix concave hull calculation

* Add a creator explorer, there should be a list of creators, then a list of collections, if there are subcollections then show those

* If there is an image in a directory, allow setting this as the collection preview, creator preview

* findamodel config files should have a model metadata dictionary field, these would always need to be alongside the model file itself, the key is the file path, and the object contains metadata for that model, such as preview image and name

* Allow rules section in a config, this allow specifying rules for how values are calculated
* Default rules should be 'folder' - the name of the parent folder, 'filename' - the filename (without the extension), 'regex' - see other details below
* When a regex rule is used the value passed into the regex is specified by the source, in the example below it would be the full path of the file (relative to the root)
  - name:
      regex:
        source: path
        expression: s|.*/([^/]*)/[^/]*$|\1|

* Determine if db.Database.ExecuteSqlRaw("PRAGMA journal_mode=WAL;"); is needed

* On the main screen show the X most recently added models (configurable via config variable)

* Add warning if there are any models off canvas when hitting save plate

* Add option to show or hide labels

* Right align the reset button

* Change the grouped/random toggle to a dropdown (mui select), and title it 'spawn order'

* When starting to test larger mdoels and concave hulls, add a new spawn order - largest first, fill gaps - that spawns the largest model, then enough small models to fit in the bound rectangle minus the area of the convex/concave hull, for this all 'fill' models should be spawned at the same x location

* Show a little bit above the top of the print plate, so that you can see clearly if any models are "out of bounds"
* If models are out of bounds, add a thick red internal dotted border - use visual not physics data to determine this

* Make the hardcoded 2mm raft clipping configurable - it should default to 2mm, but be overridable using the findamodel.yaml metadata

* Store the hull mode / spawn type / on the printing list

* Setup prettier, and have claude run it after frontend changes

* Add model name to metadata - allowing a model with multiple parts to be marked as one model
* Add 3D Print type to model, FDM / Resin / Any

* Draw sans raft hull Xmm (where X = however many the raft vertical offset was set to) in the air within the viewport. Use the same colors as the sans raft dialog below
* Remove the convex hull card, and replace it with the sans raft one below. Then rename the title to say just "Convex"

* UpdateLogger so the service/context is passed in separately from a constant as a channel, so that certain channels can be enabled or disabled, investigate logging libraries before choosing a method here
* By default any logs of the ExecutedDbCommand should be set to Debug level, and that logging channel disabled
* See if there's a VSCode serilog plugin or similar that can provide the ability to filter channels dynamically