* Do a whole debug pass on tags, I think it needs some work
=> Separate AI tag indexing from normal index, allow it to run separately (including separate cron/startup indexing)
=> On the model card, show standard tags (format, support, etc...) separately from user/ai tags
=> Allow clicking on either standard or ai tags to add that to the filtere (note: ADD to - not replace)

* Add a refresh index button to the model card
=> Is there an easy way to show the index is out of date, and only show it if it is, or does that require additional scan/work?

* Add an 'initial setup' wizard, if the environment variable isn't defined to set the model location, it allows selection of the model root and saves that out in the database
=> Allow all config/environment values that set the default values to be overriden and saved out during that config wizard step
=> Only use the appsettings/env vars for that initial setup, and use sensible default values if they're not overridden

* When indexing at startup, the population of the database does not appear to happen until all the preview images have been generated - this should be populating the database incrementally.
* Add the ability to safely cancel an index via the indexing page

* The cards in 'other parts' on the model details page show wayyyy too large (they should be the same size as the model cards on the models page)

* Some support parts aren't detected as supports - these are usually the tips of the supports

* Add note to AI to ignore orange parts of the image, or somehow filter them out before passing it over? Or render previews both with and without supports?

* Remove the close button from the some files in this list cannot be included in exported plates warning

* Remove the manage lists button on hover of "printing", and add a "switch list" button to the right of the list name header on the printing page

* Make the search column count dynamic so the page works on mobile

* Investigate how expensive downscaling meshes is - some of the meshes are 60mb plus, which takes a while streaming to the frontend when on wifi. Evaluate whether minimizing that would speed that up, or if the compute cost would take longer than over the wire. Assume a it will be used mostly on a 1gbit lan connection, but will also occasionally be used on a 20mbit internet connection, having support for both is a valid option.

* Indexing seems to take a long time to actually start processing files... 30s or so for 8 files, no idea how long for 2000 files
=> If ai generation is happening, it almost looks as if it's doing all of that before starting anything else?
=> remove the tooltip 

* Add a toggle to settings for enable description generation

* Don't reset viewport when toggling supports on/off

* Don't run ai tagging/description generation on anything that doesn't have a valid preview - using this logic, model change shouldn't be the trigger for the ai tagging/description generation to re-run, but rather the preview being changed (use modified date of the preview image vs stored description/tag generation date - if the preview is newer, re-run)