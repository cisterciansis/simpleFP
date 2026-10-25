function colors_out = get_plot_colors(plot_params_struct, num_items, param_name, default_multiple_colors, default_single_color)
% GET_PLOT_COLORS Handles color assignments for plots.
%   Retrieves color settings from plot_params_struct or uses defaults.
%   Manages cases for single color specification vs. multiple.

    if isfield(plot_params_struct, param_name) && ~isempty(plot_params_struct.(param_name))
        specified_colors = plot_params_struct.(param_name);
        if (ischar(specified_colors) || (isstring(specified_colors) && isscalar(specified_colors))) || ...
           (isnumeric(specified_colors) && isvector(specified_colors) && length(specified_colors) == 3 && (size(specified_colors,1)==1 || size(specified_colors,2)==1) ) % Single color string or RGB triplet (row or column)
            
            if isnumeric(specified_colors)
                colors_out = repmat(specified_colors(:)', num_items, 1); % Ensure N x 3
            else
                 colors_out = repmat(specified_colors, num_items, 1); % For char/string
            end

        elseif (isnumeric(specified_colors) && size(specified_colors, 2) == 3 && size(specified_colors, 1) == num_items) || ...
               (iscell(specified_colors) && length(specified_colors) == num_items) % Correct number of colors provided
            colors_out = specified_colors;
        else % Mismatch or unexpected format
            warning('grabACHAnalysisTools:get_plot_colors:ColorMismatch', ...
                    'Provided "%s" has unexpected format or count. Using default.', param_name);
            if num_items <= size(default_multiple_colors,1) && num_items > 0
                colors_out = default_multiple_colors(1:num_items,:);
            elseif num_items > 0
                colors_out = repmat(default_single_color, num_items, 1);
            else
                colors_out = []; % No items, no colors
            end
        end
    else % Not specified, use defaults
        if num_items <= size(default_multiple_colors,1) && num_items > 0
            colors_out = default_multiple_colors(1:num_items,:);
        elseif num_items > 0
            colors_out = repmat(default_single_color, num_items, 1);
        else 
            colors_out = [];
        end
    end
end