function P = scannerMatrix(position)
%SCANNERMATRIX Rotation from phantom RAS+ into scanner coordinates.
%   P = bifti.scannerMatrix(position) for a patient position code ('FFS',
%   'FFP', 'FFDR', 'FFDL', 'HFS', 'HFP', 'HFDR', 'HFDL'), or for a phantom
%   config or loaded phantom, gives the 3x3 matrix with v_scanner = P * v_ras.
%   An empty position ('' or a phantom without patient) is FFS, the identity.
%   The scanner frame has Z along B0 out of the bore and Y up (NIFTI.md).
%
%   See also bifti.scannerAffine.
    if isstruct(position)
        if isfield(position, 'config')
            position = position.config;
        end
        position = position.patient;
    end
    if isempty(position)
        position = 'FFS';
    end
    codes = {'FFS', 'FFP', 'FFDR', 'FFDL', 'HFS', 'HFP', 'HFDR', 'HFDL'};
    matrices = {eye(3), diag([-1 -1 1]), [0 1 0; -1 0 0; 0 0 1], [0 -1 0; 1 0 0; 0 0 1], ...
        diag([-1 1 -1]), diag([1 -1 -1]), [0 -1 0; -1 0 0; 0 0 -1], [0 1 0; 1 0 0; 0 0 -1]};
    match = find(strcmp(codes, position), 1);
    if isempty(match)
        error('bifti:patientPosition', 'Unknown patient position "%s", expected one of %s', ...
            position, strjoin(codes, ', '));
    end
    P = matrices{match};
end
