function affine = placedAffine(resliceTo)
%PLACEDAFFINE Voxel-to-RAS+ affine of a reslice_to grid after its move_to.
%   affine = bifti.placedAffine(resliceTo) returns the 3x4 affine the resampled
%   data is placed at (JSON.md -> reslice_to.move_to). resliceTo.affine and
%   resolution define which region of the source data is sampled; the optional
%   field moveTo places the resampled grid independently of that:
%     [] or absent  the grid stays at resliceTo.affine.
%     'center'      rotation and voxel size of affine are kept, the grid is
%                   translated so that voxel floor(resolution / 2) (zero-based)
%                   lies at the origin.
%     3x4 matrix    used as given, e.g. to place an obliquely sampled slab
%                   axis-aligned. A different voxel size than affine warns
%                   (bifti:moveToStretches): the phantom is stretched.
%
%   See also bifti.loadPhantom, bifti.readPhantom.
    affine = resliceTo.affine;
    if ~isfield(resliceTo, 'moveTo') || isempty(resliceTo.moveTo)
        return
    end
    moveTo = resliceTo.moveTo;
    if ischar(moveTo) || isstring(moveTo)
        if ~strcmp(moveTo, 'center')
            error('bifti:moveTo', 'reslice_to.move_to must be ''center'' or an affine, got ''%s''', char(moveTo));
        end
        center = floor(double(resliceTo.resolution(:)) / 2);
        affine(:, 4) = -affine(:, 1:3) * center;
        return
    end
    if ~isnumeric(moveTo) || ~(isequal(size(moveTo), [3 4]) || isequal(size(moveTo), [4 4]))
        error('bifti:moveTo', 'reslice_to.move_to must be ''center'' or a 3x4 affine');
    end
    placed = double(moveTo(1:3, :));
    sampled = sqrt(sum(affine(:, 1:3) .^ 2, 1));
    moved = sqrt(sum(placed(:, 1:3) .^ 2, 1));
    if any(abs(moved - sampled) > 1e-3 * max(abs(sampled), 1e-12))
        warning('bifti:moveToStretches', ['reslice_to.move_to has voxel size %s, but the data is ' ...
            'resliced with voxel size %s: the phantom will be stretched'], mat2str(moved, 6), mat2str(sampled, 6));
    end
    affine = placed;
end
