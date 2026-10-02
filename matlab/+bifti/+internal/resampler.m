function r = resampler(srcAffine, srcShape, dstAffine, dstShape)
%RESAMPLER Precompute density-weighted footprint resampling between grids.
%   Same algorithm as the Python, Rust and Julia implementations (see the
%   top-level README, "Reslicing"). Each output voxel averages the whole source
%   region it covers: exact box weights per axis when every output axis maps
%   onto one source axis ('separable'), midpoint quadrature over the voxel's
%   parallelepiped otherwise ('oblique'), linear interpolation along axes that
%   are not downsampled. Affines are 3x4 or 4x4 for zero-based voxel indices.
%   Apply it with bifti.internal.contract.
    maxObliqueSubsteps = 8;      % the separable path is exact and needs no cap
    axisAlignedTol = 1e-9;       % far below any real obliquity (1 deg gives ~0.017)
    srcAffine = as4x4(srcAffine);
    dstAffine = as4x4(dstAffine);
    srcShape = double(srcShape(:)');
    dstShape = double(dstShape(:)');
    if isequal(srcShape, dstShape) && norm(srcAffine - dstAffine) <= sqrt(eps) * norm(srcAffine)
        r = struct('kind', 'identity', 'shape', dstShape);
        return
    end

    m = srcAffine \ dstAffine;   % output voxel index -> world -> source voxel index
    srcAxis = zeros(1, 3);
    for a = 1:3
        column = m(1:3, a);
        [~, s] = max(abs(column));
        others = abs(column([1:s-1, s+1:3]));
        if norm(column) == 0 || any(others > axisAlignedTol * norm(column))
            srcAxis = [];
            break
        end
        srcAxis(a) = s;
    end

    if isempty(srcAxis) || numel(unique(srcAxis)) < 3
        spans = sqrt(sum(m(1:3, 1:3) .^ 2, 1));
        substeps = ones(1, 3);
        substeps(spans > 1) = min(maxObliqueSubsteps, ceil(spans(spans > 1)));
        r = struct('kind', 'oblique', 'shape', dstShape, 'm', m, 'substeps', substeps);
        return
    end

    matrices = cell(1, 3);
    for a = 1:3
        s = srcAxis(a);
        matrices{s} = axisMatrix(dstShape(a), srcShape(s), m(s, a), m(s, 4));
    end
    r = struct('kind', 'separable', 'shape', dstShape, 'srcAxis', srcAxis);
    r.matrices = matrices;
end

function a = as4x4(a)
    if size(a, 1) == 3
        a = [a; 0 0 0 1];
    end
end

function w = axisMatrix(nOut, nSrc, scale, offset)
% The (nOut, nSrc) weights of one axis; output index i (zero-based) maps to source
% coordinate scale*i + offset. Rows sum to 1 over the unclipped footprint, so an
% output voxel half outside the source keeps half its density.
    weightEps = 1e-12;  % rounding crumbs from inverting the affine, not overlap
    i = (0:nOut - 1)';
    j = 0:nSrc - 1;
    if abs(scale) > 1
        lo = min(scale * (i - 0.5), scale * (i + 0.5)) + offset;
        hi = max(scale * (i - 0.5), scale * (i + 0.5)) + offset;
        w = max(bsxfun(@min, hi, j + 0.5) - bsxfun(@max, lo, j - 0.5), 0) / abs(scale);
    else
        w = max(1 - abs(bsxfun(@minus, scale * i + offset, j)), 0);
    end
    w(w < weightEps) = 0;
end
