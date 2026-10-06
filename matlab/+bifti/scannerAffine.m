function A = scannerAffine(affine, position)
%SCANNERAFFINE Voxel-to-scanner affine of a voxel-to-RAS+ affine.
%   A = bifti.scannerAffine(affine, position) composes a 3x4 or 4x4 affine
%   with a patient position (see bifti.scannerMatrix), giving the 4x4
%   P4 * affine of NIFTI.md. A = bifti.scannerAffine(phantom, tissueName)
%   does this for a tissue of a phantom loaded with bifti.loadPhantom.
    if isstruct(affine)
        phantom = affine;
        tissue = phantom.tissues(strcmp({phantom.tissues.name}, position));
        if isempty(tissue)
            error('bifti:tissue', 'The phantom has no tissue "%s"', position);
        end
        A = bifti.scannerAffine(tissue.affine, phantom.config.patient);
        return
    end
    if size(affine, 1) == 3
        affine = [affine; 0 0 0 1];
    end
    A = blkdiag(bifti.scannerMatrix(position), 1) * affine;
end
