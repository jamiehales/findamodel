* Move the model preview images out of the database and into a cache/renders folder, with the images being labelled with the hash of the model file

* Remove all the garbage sx from every control, and tell AI to stop that shit, use themes instead, we don't want it overriding stuff that much it makes things hard to keep consistent

* Add a search page that can search by any of the fields on a model

* Expand the main page to fill the whole width of the screen

* Add a 'download all models' button to the printing list page which zips up all the model files selected, and downloads it - show a progress spinner while the zipping is happening so the user is informed. Show progress of the zip process if possible.

* Fix popping when the model viewer loads

* Reduce complexity of convex hull
* Fix concave hull calculation

* Add the following field to the model metadata when indexing - size (x/y/z dimensions of the object in mm), and spherical center of the model. Making sure to use the model service for both of these tasks so that the data is pre-transformed into a y up coordinate system. Update the model api endpoints to return this data

* Add a file explorer, where you can navigate through the file structure
* This should be a grid view, with a card for folders and a card per model, same as the model explorer
* Folders should be shown first, alphabetically, followed by all the models also sorted alphabetically
* There should be editable fields for the metadata (findamodel.yaml) in each of these folders, it should show both the local value, or the inherited value. If you edit a field it should update the json file with this data, and the corresponding database entry. Making sure to update all the children with the new calculated values.

* Add an author explorer, there should be a list of authors, then a list of collections, if there are subcollections then show those

* Rename author to creator?

* If there is an image in a directory, allow setting this as the collection preview, author preview

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

* Later, add Manage Printing Lists, which takes you to a manage printing lists page where you can add/remove and activate printing lists

* A simple search should be in the top right of the screen when on the main screen which opens a model dropdown which shows a scrollable list of models (limited to 20 or so)
