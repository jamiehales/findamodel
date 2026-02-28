* Add the following field to the model metadata - size (dimensions of the object in mm)

* Add a 3D preview to each model page (use threejs)

* Move the model preview images out of the database and into a cache/renders folder, with the images being labelled with the hash of the model file

* Add a search page that can search by any of the fields on a model

* Expand the main page to fill the whole width of the screen

* Add an "add to printing list" button, which then shows a +/- button to store how many of that model. The data should be stored referencing the models guid. This should be stored in the users local browser session for now, with possibility of adding a real user system later
* Add a link on the front page to "view printing list", which shows all the model cards of the models added to the printing list 
* Add a 'download all' button which zips up all the model files selected, and downloads it - show a progress spinner while the zipping is happening so the user is informed. Show progress of the zip process if possible.

* Calculate a convex and concave hull for each model on import, based on the y axis, using a y up coordinate system. Use existing libraries for this.

