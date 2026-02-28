* During scanning update the database on every discovered file, instead of all at the end as there may be tens of thousands of files

* Create a json based metadata configuration file that can be placed at any level above the model file to the top of the directory - this will populate the metadata for each model
* The configuration should be composed by starting at the closest file, then iterating through parent directories and combining the data until all required fields are full. The first value found takes priority for that field.
* The fields that should be stored are.... Author, Collection,  ????
* Any fields that aren't set by the time the root is filled are null/empty (as relevant for the field type)
* Cache the hash of these configuration files, and if any have changed then add a function stub for handling updates of these

