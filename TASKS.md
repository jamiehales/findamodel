* Switch data fetching to react-query

* Move the model preview images out of the database and into a cache/renders folder, with the images being labelled with the hash of the model file

* Add a search page that can search by any of the fields on a model

* Expand the main page to fill the whole width of the screen

* Add an "add to printing list" button, which then shows a +/- button to store how many of that model. The data should be stored referencing the models guid. This should be stored in the users local browser session for now, with possibility of adding a real user system later
* Add a link on the front page to "view printing list", which shows all the model cards of the models added to the printing list 
* Add a 'download all' button which zips up all the model files selected, and downloads it - show a progress spinner while the zipping is happening so the user is informed. Show progress of the zip process if possible.

* Fix popping when the model viewer loads

* Add a new endpoint that instead of directly returning the stl file has the backend load the model with all transformations applied, and returned in a format that threejs can read easily
* Update model viewer to use this new transformed API call, and remove all specific handling of obj and stl. Remove all transformations and modifications of the vertices as they should now already be transformed by the backend before receiving the vertex data.
* Refactor all the frontend api calls to use react-query

* Fix concave hull calculation

* Add the following field to the model metadata when indexing - size (x/y/z dimensions of the object in mm), and spherical center of the model. Making sure to use the model service for both of these tasks so that the data is pre-transformed into a y up coordinate system. Update the model api endpoints to return this data

* Modify the scale of the grid in the model viewer, one square should be one inch, with quarter inch subdivisions

* Add a file explorer, where you can navigate through the file structure
* This should be a grid view, with a card for folders and a card per model, same as the model explorer
* Folders should be shown first, alphabetically, followed by all the models also sorted alphabetically
* There should be editable fields for the metadata (findamodel.json) in each of these folders, it should show both the local value, or the inherited value. If you edit a field it should update the json file with this data, and the corresponding database entry. Making sure to update all the children with the new calculated values.

* Add an author explorer, there should be a list of authors, then a list of collections, if there are subcollections then show those

* Rename author to creator?

* If there is an image in a directory, allow setting this as the collection preview, author preview

* findamodel config files should have a model metadata dictionary field, these would always need to be alongside the model file itself, the key is the file path, and the object contains metadata for that model, such as preview image and name

* Allow rules section in a config, this allow specifying rules for how values are calculated, the value passed into the regex is specified by the source, in the example below it would be the full path of the file (relative to the root)
  - name:
      regex:
        source: path
        expression: s|.*/([^/]*)/[^/]*$|\1|

* Switch json configuration files to yaml

* Determine if db.Database.ExecuteSqlRaw("PRAGMA journal_mode=WAL;"); is needed

* Allow overriding path to models database via config variable
