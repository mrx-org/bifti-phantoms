# NIfTI Phantom file format

NIfTI phantoms is a universal, flexible and implementation-agnostic format for storing MRI simulation data.
It is based on two widely used file formats:
- NIfTI for volumetric data, easy to view, modify, store in many applications and programming languages
- JSON for phantom definitions - human readable, basically supported everywhere

The NIfTI phantom format specifies how to store the data, how to structure the JSON file and how to reference this data.
Its goal is to be easy enough to use that configuring phantoms for different experiments is easier done by creating variations of JSON configurations than to modify from code - making experiment data exchangable and reproducible.

This repository contains the [specification](SPEC.md), a [JSON schema](nifti-phantom-v1.schema.json) of the JSON files and a reference implementation of phantom loading in Python.


# NIfTI Phantom registry

We strive to make phantom data exchangable between groups.
This is only partly achieved by a universal specification - a shared place to store and share phantoms helps as well.
This is why this repository also contains a phantom registry - a list of public NIfTI phantoms usable by anyone.
The phantom data itself is stored by providers like [Zenondo](https://zenodo.org/).
The registry is stored here, in the form of a [list of available phantoms](registry.json).

Anyone is welcome to add their own phantoms to share by uploading to a provider of their own choosing and adding it to the available list via an PR.
