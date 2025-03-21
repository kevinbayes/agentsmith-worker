import {
    Box,
    styled,
} from "@mui/material";

export const FullPageContainer = styled(Box)(({ theme }) => ({
    minHeight: "100vh",
    maxHeight: "100%",
    height: "100vh",
    width: "100%",
}));
