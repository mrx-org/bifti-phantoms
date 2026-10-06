function y = contract(r, x)
%CONTRACT Footprint sum of X through a resampler from bifti.internal.resampler.
    switch r.kind
        case 'identity'
            y = x;
        case 'separable'
            for d = 1:3
                x = contractDim(x, r.matrices{d}, d);
            end
            % Still indexed by source axis; output axis a is source axis srcAxis(a).
            y = permute(x, r.srcAxis);
        case 'oblique'
            y = contractOblique(r, x);
    end
end

function y = contractDim(x, matrix, d)
    perm = [d, setdiff(1:3, d)];
    front = permute(x, perm);
    shape = [size(front, 1), size(front, 2), size(front, 3)];
    y = reshape(matrix * reshape(front, shape(1), []), [size(matrix, 1), shape(2:3)]);
    y = ipermute(y, perm);
end

function y = contractOblique(r, x)
    [i1, i2, i3] = ndgrid(0:r.shape(1) - 1, 0:r.shape(2) - 1, 0:r.shape(3) - 1);
    output = [i1(:), i2(:), i3(:)]';
    offsets = @(n) ((1:n) - 0.5) / n - 0.5;
    [d1, d2, d3] = ndgrid(offsets(r.substeps(1)), offsets(r.substeps(2)), offsets(r.substeps(3)));
    y = zeros(size(output, 2), 1);
    for q = 1:numel(d1)
        source = r.m(1:3, 1:3) * (output + [d1(q); d2(q); d3(q)]);
        y = y + trilinear(x, source + r.m(1:3, 4));
    end
    y = reshape(y / numel(d1), r.shape);
end

function v = trilinear(x, c)
% Trilinear samples at zero-based source coordinates c (3 x n); outside reads as 0.
    shape = [size(x, 1), size(x, 2), size(x, 3)];
    base = floor(c);
    frac = c - base;
    v = zeros(size(c, 2), 1);
    for corner = 0:7
        delta = bitget(corner, 1:3)';
        index = base + delta + 1;
        weight = prod(delta .* frac + (1 - delta) .* (1 - frac), 1)';
        inside = all(index >= 1 & index <= shape', 1)';
        linear = sub2ind(shape, index(1, inside), index(2, inside), index(3, inside));
        v(inside) = v(inside) + weight(inside) .* x(linear(:));
    end
end
