* Add a "parent folder" folder to each explorer page, where it isn't the root

* Fix metadata directory scan on startup to be asynchronous (and uncomment it in ModelIndexerService)
* Add a boolean "index" field to findamodel.yaml which defines whether models found within that folder should be indexed, it should be possible for folders at a deeper level to be marked to be scanned which are excluded, so all files and folders still need scanning. If a model previously marked as indexable is removed and/or now marked as not to index, remove it from the database if it's not already
* Add indexing requests to a queue - handle them asynchronously one by one in order, and add an icon to the top right when indexing is happening, clicking it should show all the indexing request
* If an indexing request is already in the queue and is requested again, bump it to the top of the queue

* Add database migration support

* Remove all the garbage sx from every control, and tell AI to stop that shit, use themes instead, we don't want it overriding stuff that much it makes things hard to keep consistent

* Add a filters section to the main page that can search by any of the fields on a model (based on the calculated values from findamodel.yaml), it should update the shown model results in real time

* Expand the main page to fill the whole width of the screen

* Add a 'download all models' button to the printing list page which zips up all the model files selected, and downloads it - show a progress spinner while the zipping is happening so the user is informed. Show progress of the zip process if possible.

* Reduce complexity of convex hull
* Fix concave hull calculation

* Add the following field to the model metadata when indexing - size (x/y/z dimensions of the object in mm), and spherical center of the model. Making sure to use the model service for both of these tasks so that the data is pre-transformed into a y up coordinate system. Update the model api endpoints to return this data

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

* Add a sidebar that has the following items:
  - View Printing List - shows the name of the active printing list below it, if you click it it takes you to the printing list
  - Browse Models - opens the file view
  - Advanced Search - opens the search view

* A simple search should be in the top right of the screen when on the main screen which opens a model dropdown which shows a scrollable list of models (limited to 20 or so)

* Add warning if there are any models off canvas when hitting save plate

* Fix bug where the layout resets if save plate is clicked

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

* Draw sans raft hull Xmm (where X = however many the raft vertical offset was set to) in the air within the viewport. Use the same colors as the sans raft dialog below
* Remove the convex hull card, and replace it with the sans raft one below. Then rename the title to say just "Convex"
