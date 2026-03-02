* Add a filters section to the main page that can search by any of the fields on a model (based on the calculated values from findamodel.yaml), it should update the shown model results in real time
* A new api /query endpoint will need to be added, as well as a QueryService to discover these models, but still return the metadata via ModelService
* The results should be limited based on a supplied parameter (default to 25), but support "show more" at the bottom to show more
* Where the fields are multi-choice a dropdown should be shown, defaulting to an empty/ignore check
* For booleans they should show as three state checkboxes
