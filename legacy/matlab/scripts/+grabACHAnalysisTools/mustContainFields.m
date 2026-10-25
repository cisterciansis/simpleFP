function mustContainFields(s, fields)
% MUSTCONTAINFIELDS Custom validation function for arguments block.
%   Checks if a struct 's' contains all fieldnames specified in the
%   cell array 'fields'. Throws an error if not.

    if ~isstruct(s)
        eid = 'grabACHAnalysisTools:mustContainFields:notStruct';
        msg = 'Input must be a struct.';
        throwAsCaller(MException(eid,msg));
    end
    if ~iscellstr(fields) && ~isstring(fields) % Allow cell array of chars or string array
        eid = 'grabACHAnalysisTools:mustContainFields:invalidFieldsArg';
        msg = 'Fields argument must be a cell array of strings or a string array.';
        throwAsCaller(MException(eid,msg));
    end

    for k = 1:length(fields)
        % Ensure field is char for isfield, robust for string arrays from arguments block
        if iscell(fields) && ischar(fields{k}) % Cell of char vectors
            current_field = fields{k};
        elseif isstring(fields) % String array
            current_field = char(fields(k));
        else
            eid = 'grabACHAnalysisTools:mustContainFields:invalidFieldTypeInCell';
            msg = sprintf('Field %d in fields list is not a character vector or string scalar.', k);
            throwAsCaller(MException(eid,msg));
        end
        
        if ~isfield(s, current_field)
            eid = sprintf('grabACHAnalysisTools:mustContainFields:missingField:%s', current_field);
            msg = sprintf('Struct input must contain the field "%s".', current_field);
            throwAsCaller(MException(eid,msg));
        end
    end
end